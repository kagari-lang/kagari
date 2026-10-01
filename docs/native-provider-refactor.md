# Native Provider and Contract Refactor Plan

Status: queued; planning only. Implementation starts after
[standard-library and HIR integration](stdlib-hir-refactor.md) completes ST06,
including its behavior matrix, carried-error resolution and final checks.
This plan does not change the active ST00-ST06 scope or authorize starting this
migration during that work. The [roadmap](implementation-roadmap.md) records the
ordering. Re-audit the completed predecessor before selecting implementation work.

## Objective and acceptance boundary

Treat standard-library native functions as registrations from an engine-installed
provider. Engine and host providers share declaration, binding, contract checking,
linking, invocation and callback/resumption mechanisms. Their authority and
supported representations remain explicit and may differ.

The maintainability requirement is concrete: adding a native function that uses
existing language/runtime capabilities changes only its declaration, implementation,
provider registration and behavioral tests. It must not require adding its identity
to HIR policy, compiler lowering, ABI verification, bytecode dataflow, VM dispatch,
or a central continuation-family enumeration. Exercise this requirement with a
separately compiled embedding consumer, not just another built-in function.

Additional metadata is welcome when it describes reusable facts: signatures,
generic parameters, override policy, callable requirements, effects, capabilities,
passing styles and execution requirements. Metadata that says "use the special
ArraySortBy path" merely relocates the existing coupling and fails acceptance.

## Relationship to the predecessor

The predecessor establishes ordinary HIR declarations, portable checked native
calls and runtime-owned standard algorithms. Reuse those results. This follow-up
removes remaining knowledge of individual library methods from generic machinery
and exposes the same invocation capabilities to registered host implementations.

The predecessor permits existing synchronous host callbacks alongside engine-only
continuations. It does not require an extensible continuation registration API or
the no-core-edit extension test above. Moving algorithms out of the compiler alone
therefore does not satisfy this plan.

Specifications remain authoritative, especially [host interoperability](spec/host-interop.md),
[failure semantics](spec/failure-semantics.md), [value semantics](spec/value-semantics.md),
[artifacts](spec/artifacts.md), [module loading and reload](spec/module-loading.md) and
[standard declarations](spec/standard-declarations.md). Update affected specifications
with the phase that changes their API/contracts, rather than treating this proposal
as a description of implemented behavior.

## Audit inputs

The following evidence was inspected at revision `136ec596`. It is a starting
inventory, not a promise that these paths survive ST06. NR00 must classify the
post-ST06 code and record the final replacement owner for each remaining case.

| Area | Observed coupling | Intended replacement |
| --- | --- | --- |
| [HIR method defaults](../crates/kagari-hir/src/aggregates/traits.rs) | `NativeDefaultMethod` lists decide override eligibility | Checked declaration policy, applied uniformly |
| [Installed bindings](../crates/kagari-hir/src/native/stdlib/functions.rs) | Marker installation selects intrinsic/default/protocol enums | Provider-qualified binding lookup against an offline contract set |
| [Callable identities](../crates/kagari-abi/src/callable.rs) | Public native methods use central engine operation families | Provider-owned binding identities, separate from language primitives |
| [Native contract checks](../crates/kagari-abi/src/native_import/contract.rs) and [callback checks](../crates/kagari-abi/src/native_import/contract/callbacks.rs) | Per-method signature, callback and constraint branches | Generic instantiation and trusted registration-contract comparison |
| [Effects](../crates/kagari-abi/src/effects.rs) and [import resolution](../crates/kagari-abi/src/native_import.rs) | Central method lists classify mutation, allocation and resumability | Trusted descriptor facts and linked entry capabilities |
| [Witness validation](../crates/kagari-abi/src/native_import/linked/protocols.rs) | Iterator/Ord/aggregation-specific method selection | Explicit required callable applications and generic witness verification |
| [MIR calls](../crates/kagari-mir/src/instruction.rs) and [bytecode access checks](../crates/kagari-bytecode/src/access.rs) | Parallel intrinsic targets and per-operation type/access propagation | Uniform public native calls; narrowly separated primitive instructions |
| [Runtime continuation factory](../crates/kagari-runtime/src/native/mod.rs) | Closed Enum/Iterator states and aggregation entry policy | Provider entry factories and a common invocation lifecycle |
| [Interface construction](../crates/kagari-runtime/src/objects.rs) | Omitted method slots recognized through Engine/TraitDefault identity | Explicit resolved default-callable metadata |
| [VM invocation](../crates/kagari-vm/src/executor/dispatch.rs) | Engine and Host have different callback entry paths; intrinsic guard switches | Linked entry invocation and generic frame-owned resources |
| [Host declaration types](../crates/kagari-common/src/host_interface/value_type.rs) and [host callbacks](../crates/kagari-runtime/src/host.rs) | No function-valued declaration type; registration returns synchronously | Checked callable parameter support and public resumable registration |

Do not classify every `Standard*` reference as debt. Storage layout, primitive
integer semantics, GC tracing, built-in enum representation and language protocols
can require engine knowledge. Library implementation modules can contain their own
algorithm-specific branches. The violation is method-specific policy in otherwise
generic compilation, validation, linking or invocation infrastructure.

## Target design

### One contract model, provider-owned implementations

Keep three responsibilities separate without maintaining three independent public
signature catalogs:

1. Declarations supply public signatures, generic bounds, docs and method policy.
   Installed `.kgr` declarations use ordinary HIR. Offline host declarations use
   the same checked callable facts without executing Rust handlers.
2. A trusted provider contract describes what the implementation accepts and
   requires. It carries the portable binding identity/version, representation and
   callable requirements, effects, authority and passing rules. Validate the
   declaration against it using common type/constraint machinery.
3. A runtime registration pairs that contract with a direct entry or invocation
   factory. Algorithm state and implementation-specific validation live here.

Source-free compilation and artifact validation use immutable contract snapshots
without executable handlers. A runtime installs the matching implementation set.
The same registration contract is exported for offline use and checked at runtime;
do not hand-maintain a second argument-count or callback-signature table.

For `.kgr` APIs, HIR remains the source of public semantic signatures. Typed Rust
adapters or provider-owned descriptors may state native requirements and check
agreement. They must not parse `.kgr` independently, generate another source type
language or cause ABI to depend on HIR. Registration macros are optional ergonomics;
an explicit usable Rust API is required first.

### Binding identity, linking and trust

Use a provider-qualified durable binding key and an explicit contract version.
Keep source declaration identity, binding identity and module-local import slot
distinct. Prefer existing identity facilities where they meet this contract; do
not require a new globally allocated numeric ID space. If numeric wire keys are
chosen, assign them explicitly rather than using Rust enum declaration order.

Keys are identities, not authority proofs. Resolve against the contract/implementation
sets installed by the embedding application. Reject duplicate keys, unsupported
versions, missing providers and contract mismatches before execution. Namespace or
attribute spelling cannot acquire engine capabilities or replace an installed
standard function. Hashes accelerate lookup; full relevant contract comparison
establishes agreement.

Link each import once to a module-local entry slot. Execution uses that linked
slot without source-name lookup, per-call contract reconstruction or searching
standard-method catalogs. Freeze the installed provider generation for loaded
programs; keep its implementation owner alive across callbacks and hot reload.
Registering a replacement affects newly linked programs, never suspended calls.

Artifact bytes cannot supply their own trusted effects or validation authority.
Generic structural verification can precede provider availability, but executable
preparation must validate imports against the installed trusted contract snapshot.
No execution seal may imply that an unresolved native implementation is safe to
enter. Cache provider-dependent validation against contract identities/versions;
do not reuse it for another installation with different contracts. Runtime linking
still checks actual handlers and provider-specific authority. Specify these seals
and preparation APIs in NR01 before migrating callers.

### Common metadata consumed by generic layers

| Fact | Producer and use |
| --- | --- |
| Signature and concrete application | HIR supplies ordered types, generic substitution and bounds; lowering transports them; validators check them generically |
| Method policy | Trusted declaration installation supplies override/default facts; HIR never derives them from a function ID |
| Required callables | Contract declares receiver/type relationships and required trait member identities; HIR selects witnesses; portable imports carry concrete callable applications |
| Effects and access | Provider contract supplies conservative allocation/mutation/callback/host effects and required access; optimizers and validators use verified facts |
| Passing and authority | Provider policy carries capabilities, owned/borrowed passing, no-escape rules and candidate-initialization restrictions |
| Execution entry | Registration supplies a direct entry or resumable factory; linked handles retain the entry and its owner |
| Roots and work accounting | Runtime context offers explicit retained values, guarded resources and charge/poll operations; implementations use them during their algorithms |

Required-callable metadata must express the existing Iterator, Ord, Hash/Eq,
conversion and aggregation uses without a selector like "if Sum then choose this
trait" in a generic linker. Reuse ABI type substitution and proof machinery rather
than inventing an unbounded contract scripting language. Any extension needed to
the metadata vocabulary requires a capability-level design and tests, not one new
case per library function.

Direct entries must declare conservative effects for any supported synchronous
reentry. Resumable entries must include callback effects rather than appearing
pure because their own Rust state transitions only manipulate local buffers.

### Shared invocation and callback API

Both provider kinds can register synchronous entries and resumable implementations.
A common driver accepts bounded progress, a checked callable request, completion
or failure. The implementation owns its algorithm state and resumes with a validated
callback outcome. The driver must not enumerate Option/Iterator/Sort families.
Trait objects or an equivalent erased entry/factory are implementation choices;
a new central enum variant for every extensible algorithm is not acceptable.

The public host surface must be complete end to end: offline function-valued
parameter declarations, bounded encoding, HIR checking, portable imports, argument
conversion to a checked callable handle, runtime entry and result validation.
Raw `Value` storage alone does not provide this capability. Native contexts supply
rooted callable/value handles and frame-owned guard facilities. Returning a
callback request must not retain a Rust heap/table borrow or an escaped host lease.
Reacquisition after callback return follows the existing capability/borrow checks.

Keep existing synchronous host reentry behavior, including supported handling of
ordinary nested traps. Make ordinary callback failure delivery an explicit part
of the common protocol; built-in implementations propagate it according to their
existing semantics. Cancellation, exhaustion and quarantine remain sticky and
cannot be converted into successful continuation. Cleanup drops the native suffix
and engine-managed resources without arbitrary script execution or fuel charges.

Callbacks execute on the current bounded session stack with retained program
generations, debugger origins and call-depth accounting. A provider cannot create
a new session to reset budget or switch to the latest module. Cross-provider calls
use the same entry machinery. Algorithm state that survives one call, such as a
lazy iterator, must also use a traceable owned lifecycle; do not hide it in a new
global standard-function state table.

Resumption for a nested callback does not introduce language `async`, Rust futures,
cross-thread suspension or arbitrary open Rust generic instantiation. Native
generic contracts continue to support existing engine uses; public host APIs must
at least register concrete callable signatures. Follow the existing host generic
registration policy unless a separately justified capability change is recorded.

The separate [async execution proposal](async-execution-design.md) builds on these
contracts after NR05 acceptance. It distinguishes nested callback resumption from
returning control to the host while external IO is pending. This plan must not add
task scheduling, speculative waiting variants or Rust executor dependencies for
that future work; async is not required to satisfy NR00-NR05.

### Ownership and dependencies

- ABI owns portable generic contracts, bounded codecs, substitution and validation
  interfaces. It must not own a central public standard-method enumeration or
  depend on runtime handlers to validate a contract snapshot.
- HIR owns checked signatures, method policy and witness selection. Compiler owns
  generic call lowering; MIR/bytecode own their formats and generic verification.
- Runtime owns registration/linking, invocation resources and implementation entry
  interfaces. Standard algorithms live in focused built-in provider modules.
- VM and codegen consume verified call/entry contracts. SDK installs matching
  offline contracts and runtime providers and exposes public registration APIs.
- `kagari-stdlib` continues to own source packaging only. It must not acquire runtime
  dependencies merely to house Rust implementations.

At NR00 choose the physical home for engine contract descriptors so both offline
validation and runtime registration can consume them without reversing dependencies.
A populated source-free provider-contract module or crate can be justified by an
actual cycle; do not create empty crates or put policy into `common` to hide it.
Keep compiler-without-source, runtime, VM, bytecode and backends independent of
HIR/syntax/stdlib production dependencies. Preserve the existing codegen/runtime
separation. Built-in provider installation may be composed centrally; core dispatch
and verifier logic may not branch on individual registered keys.

### Primitives and behavior preservation

Classify remaining raw intrinsic targets after ST06. Public library functions must
use the common native call path. Genuine checked arithmetic, field access, storage
or GC primitives may remain dedicated operations, with a bounded documented
inventory and their own validation. Do not retain a second public standard-call
route by renaming `StandardIntrinsic` to `NativeOperation`.

Preserve stable sort, once-only key extraction, retain visitation order, mutation
guards and prepare-before-commit behavior. These are implementation/specification
contracts, not properties inferred from being a standard function. Preparation
protects collection structure/slots; it does not roll back callback side effects
or mutations inside shared element objects.

This track does not replace prepared operations with Rust's partially mutating
failure semantics, promise native-sort speed, or expand JIT coverage. Preserve
logical charges and callback-visible failure positions established by ST06.
Any later algorithm/charge change needs separate semantic and performance work.
The queued [execution policy refactor](execution-policy-refactor.md) owns that
later change after NR05: installation-based access and coarse runaway protection.
Its permission/charging simplification does not weaken this track's current
preservation requirements or remove provider contract/lifetime validation.
Arbitrary trusted Rust code cannot be forcibly preempted by descriptor metadata;
document the cooperative charging/polling obligation and enforce it in engine
implementations and the public resumable API's managed transitions.

## Ordered execution phases

### NR00 — Post-ST06 inventory and baseline

- [ ] Confirm ST06 final acceptance and record its commit and feature matrix.
- [ ] Re-audit method-specific branches, contract duplication, provider asymmetries
  and primitive exceptions. Map each finding to NR01-NR05 in this ledger.
- [ ] Select descriptor ownership, trust/seal boundaries and the public callback
  representation without creating dependency cycles.
- [ ] Run baseline checks and representative direct/callback workloads; record
  toolchain, machine, profile, features, default parallelism, cache state and input.

Exit: every remaining coupling has an owner; the measured baseline and concrete
consumer examples define behavior to preserve.

### NR01 — Generic provider contracts and source-free linking

- [ ] Implement provider-qualified keys, immutable offline contracts and runtime
  registrations with duplicate/version/authority checks.
- [ ] Define concrete callable/witness requirements and conservative effects using
  existing ABI types; establish provider-dependent verification and cache rules.
- [ ] Complete one direct function through declaration, compilation, encoded
  artifact, trusted validation, runtime linking and execution for both providers.
- [ ] Update affected wire versions and consumers; reject superseded products.

Exit: an external consumer registers a direct function through the common path
without adding a standard/engine ID enum or verifier case.

### NR02 — Declaration policy and generic call consumers

- [ ] Replace method-ID override/default rules with checked declaration properties.
- [ ] Carry required concrete callable applications; remove standard-family witness
  selection from generic linkers and validators.
- [ ] Migrate HIR, compiler, MIR/bytecode and interface defaults to common contracts;
  distinguish primitive operations from public native calls.
- [ ] Keep diagnostics, tooling and offline host declaration round trips intact.

Exit: ordinary method/call validation depends on contracts and selected targets,
not membership in a standard-method list.

### NR03 — Public resumable native registration

- [ ] Implement function-valued host declaration/encoding/checking support and
  scoped, rooted callable handles through the existing offline-to-runtime path.
- [ ] Expose common direct/resumable entry registration and continuation lifecycle
  with frame-owned roots/guards, validated outcomes and bounded progress.
- [ ] Migrate a built-in callback operation and an external host implementation
  onto the same driver, including cross-provider nested callbacks.
- [ ] Verify ordinary failure, sticky termination, GC, borrow conflicts, synchronous
  reentry, debugger origins and generation retention for the public API.

Exit: host code can implement a callback-bearing function without modifying a
runtime continuation enum, generic factory, compiler or VM.

### NR04 — Migrate built-in registrations and remove central method policy

- [ ] Migrate all remaining built-in families and lazy state to provider-owned
  descriptors, entries and invocation state without changing their algorithms.
- [ ] Remove per-method direct/resumable allowlists, effects switches, public native
  signature switches and standard-family factories from generic infrastructure.
- [ ] Remove obsolete intrinsic public-call variants and consumers; retain only
  the reviewed primitive inventory with precise ownership and validation.
- [ ] Consolidate implementation requirements with registrations, deleting the
  superseded catalogs and paths rather than retaining forwarding adapters.

Exit: built-in algorithms remain ordinary users of the common native mechanism;
their private implementation branches do not leak into generic consumers.

### NR05 — Extension proof and final acceptance

- [ ] In an isolated external consumer, add a direct function, a callback-bearing
  function and a function that invokes a supplied checked callable more than once.
  Change only consumer declarations, implementation, registration and tests.
- [ ] Exercise built-in trait defaults and generic witnesses, plus a new built-in
  registration using existing capabilities, without edits to generic core logic.
- [ ] Complete the matrix below, dependency/feature audits, specification updates
  and encoded fixture regeneration; resolve all carried errors.
- [ ] Repeat baseline measurements and report dispatch, allocations, memory and
  callback behavior without asserting improvements unsupported by measurements.

Exit: both providers satisfy the extension boundary, all checks pass, and no
method-specific infrastructure debt is waived as merely "metadata".

## Verification matrix

| Boundary | Required evidence |
| --- | --- |
| Extension locality | External consumer adds direct and resumable entries without editing core crates; built-in addition touches only provider-owned declaration/implementation/registration/tests |
| Source semantics | Ordinary signature checking, callable parameter inference, method syntax, generic applications, defaults/overrides, docs and declaration origins remain correct |
| Trust and decoding | Reject unknown provider/key/version, duplicate registrations, forged effects/authority/signatures, malformed callable types, missing/swapped witnesses and mismatched installed contracts |
| Offline products | Compile/check using contracts without handlers; encoded artifacts execute without source/HIR; absent provider prevents executable preparation; cache seals cannot cross incompatible contract sets |
| Invocation | Direct, script-to-script, engine-to-script, host-to-script and nested cross-provider calls use correct arguments/results and preserve once-only evaluation |
| Failure/resources | Callback trap, allocation failure, every relevant budget cut, cancellation, quarantine and reentry release the correct roots/guards/scopes; no successful recovery from sticky termination |
| GC and borrows | GC threshold one, rooted callback captures, escaped/foreign callable handles, borrow conflicts and repeated suspend/resume do not expose invalid values or retained Rust borrows |
| Reload | Old callbacks and native entries keep their implementation/dependency generations alive; replacement registrations do not change active calls |
| Collections/lazy state | Preserve stable ordering, callback counts, alias visibility, structural guards, commit guarantees and early iterator cleanup |
| Backends/features | Existing source/artifact routes, supported direct JIT cases and verified fallback remain valid; offline and source-disabled SDK consumers retain dependency boundaries |

Reuse existing native continuation/iterator, prepared collection, host interface,
offline composite, error, reload and artifact suites. Add behavior/boundary tests,
not tests that merely mirror descriptor layout. A grep audit is useful evidence,
but manually review remaining dispatch, macro expansions and primitive exceptions.

Final implementation checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p kagari-cli --features jit
git diff --check
```

Also run the dependency/feature consumers required by the completed ST06 matrix.
Use workspace profiles, default `target` and Cargo parallelism. Keep transient
logs under ignored `target/`; record durable results and reproduction commands here.

## Checkpoint and progress policy

NR00-NR05 are ordered implementation phases after activation. Complete vertical
producer-to-consumer paths in cohesive checkpoints; avoid speculative compatibility
layers. Attempt affected checks at migration boundaries. If an intermediate build
break is unavoidable, record the command, representative diagnostic, cause and
owning follow-up phase in this ledger and disclose it in the commit body. Do not
weaken validation/tests or claim phase acceptance while its required checks fail.

Implementation commits use Conventional Commits and `Native-Step: NRxx` trailers;
mark breaking API/artifact changes with `!`. Apply repository structural review and
run `git diff --check` at each checkpoint. NR05 requires all carried failures to be
resolved. Do not reopen completed ST phase ledgers for this follow-up.

## Progress ledger

- 2026-09-30: Drafted the queued follow-up at `136ec596` after a read-only architecture
  audit. The user requested a plan to execute after standard-library/HIR integration,
  not implementation now. Recorded the no-core-edit extension criterion, trusted
  provider contract model, public callback declaration gap, and shared continuation
  requirements. All NR phases remain unstarted; no runtime/performance result is
  claimed. Local-link and whitespace validation passed for the planning checkpoint;
  no Rust build or runtime tests were required for these documentation-only edits.
- 2026-09-30: Linked the separate async execution proposal as a design-only follow-up
  after NR05. Clarified that nested callback continuations do not require external
  wait scheduling in this migration. All NR phases remain unstarted; this update
  changes no implementation scope and remains uncommitted at the user's request.
