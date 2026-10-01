# Native Provider and Contract Refactor Plan

Status: data-contract design active; no NR implementation phase is accepted.
The 2026-10-01 user direction changes the previous ST06 prerequisite: design the
HIR/MIR native boundary first, replace the old standard implementation paths and
prove a small standard-library slice before restoring the remaining algorithms.
[Standard-library and HIR integration](stdlib-hir-refactor.md) remains an interim
migration checkpoint with final combined acceptance and matched measurements open.
Those obligations carry into NR final acceptance; they are not claimed complete.
The [roadmap](implementation-roadmap.md) records the revised ordering. This design
checkpoint removes no implementations and changes no executable semantics.

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
inventory, not a promise that these paths survive the current checkpoint. NR00
must classify the replacement inputs and record an owner for each remaining case.

| Area | Observed coupling | Intended replacement |
| --- | --- | --- |
| [HIR method defaults](../crates/kagari-hir/src/aggregates/traits.rs) | `NativeDefaultMethod` lists decide override eligibility | Checked declaration policy, applied uniformly |
| [Installed bindings](../crates/kagari-hir/src/native/stdlib/functions.rs) | Marker installation selects intrinsic/default/protocol enums | Provider-qualified binding lookup against an offline contract set |
| [Compiler native applications](../crates/kagari-compiler/src/source/lower/expr/native_contracts.rs) and [defaults](../crates/kagari-compiler/src/source/lower/expr/native_defaults.rs) | Method-specific branches choose source/destination, key, lazy and trait witness applications | Contract-owned callable requirements, checked HIR selections and generic bounded materialization |
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

Classify remaining raw intrinsic targets at the replacement checkpoint. Public library functions must
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

### Data-first replacement design (2026-10-01)

Rust sketches below describe responsibilities, not committed public type names or
a compiling API. Reuse existing identities, HIR/ABI types, substitutions and proofs.
Keep the expression tree, type system, CFG and storage representation while
replacing the callable boundary.

#### HIR declarations and applications

Source and offline host declarations produce the same checked signature. Keep
declaration metadata separate from per-call applications. Existing owners retain
documentation, visibility, provenance and parameter bindings.

```rust
struct CheckedCallableDecl {
    definition: DefinitionId,
    signature: ResolvedSignature,
    implementation: Implementation,
}

enum Implementation {
    Required,
    Script(BodyId),
    Native(InstalledContractId),
}

struct MethodPolicy {
    override_allowed: bool,
}

struct CheckedCall {
    target: CheckedCallTarget,
    substitution: TypeSubstitution,
    signature: AppliedCallSignature,
    required_callables: Vec<CheckedCallableRequirement>,
    coercions: Vec<ArgumentCoercion>,
}

struct CheckedCallableRequirement {
    requirement: RequirementId,
    receiver: TypeId,
    interface: NominalType,
    member: DefinitionId,
    application: AppliedCallSignature,
    type_arguments: Vec<TypeId>,
    selection: MethodSelection,
}
```

`InstalledContractId` references trusted immutable snapshot input; it is not a
runtime slot or serialized authority token. Its contract owns the durable binding
key. No standard function/default/protocol enum belongs in this implementation
model. Direct versus resumable execution does not change ordinary call typing.

Trait defaults use the declaration's implementation and explicit method policy:
Required awaits selection; Script and Native can supply defaults. Override policy
must not be inferred from a binding key. Local rebinding writeability, collection
access and host passing styles remain distinct.

`CheckedCallTarget` distinguishes a declaration application, a selected trait member
and a callable value. Receiver/callee and arguments retain existing once-only
evaluation order. Callback parameters use ordinary function types and existing
checked callable adapters. Coercions retain the declared parameter contract rather
than substituting the argument expression's type for it.

`MethodSelection` records a checked impl application, a dynamic interface member,
or an obligation from enclosing generic bounds. Generic bodies can retain symbolic
obligations until monomorphization. Compiler discharges them using bounded generic
selection over checked declarations, without inspecting syntax or matching standard
binding identities to decide which methods are needed.

Provider contracts declare callable requirements using type relationships, applied
trait/member identities, member generic arguments and existing associated projections.
For example, `T: Ord` proves applicability; `Ord::cmp(T, T) -> Ordering` identifies
an implementation dependency. Lowering cannot infer the second from a standard
function name. Requirements contain no arbitrary resolver code or selectors such
as `SortWitness` and `CollectDestination`.

#### ABI and provider contracts

ABI owns reusable portable contract vocabulary and verification interfaces. A
source-free engine contract owner supplies built-in descriptors and depends on ABI;
ABI and generic verifiers do not depend on that concrete owner. Offline validation
and runtime registration consume the same descriptors. This owner imports neither
syntax/HIR nor runtime handlers. Decide its physical module/crate in NR00 using
the actual dependency graph; do not put a public method catalog back into ABI.

Contracts carry provider-qualified durable binding keys, explicit versions,
supported signature/representation relationships, callable requirements,
conservative effects, authority and passing/resource rules. Reuse ABI templates
and substitution. `.kgr` remains the public semantic signature source; descriptors
constrain native implementation support and are checked against that signature.
Do not generate or hand-maintain another public standard API catalog.

Requirement IDs name dependencies inside a versioned contract; they are not
method-family IDs or authority proofs. Verify the complete requirement set,
member identities, concrete signatures and substitutions. Reject missing,
duplicate or substituted requirements. Hashes only accelerate contract comparison.

#### Concrete MIR callables and imports

Store callable metadata in module tables instead of embedding separate Engine/Host
contracts in each instruction. Static script/native targets share one callable
table; dynamic calls remain explicit.

```rust
struct ConcreteCallable {
    identity: ConcreteFunctionIdentity,
    signature: SignatureId,
    entry: CallableEntry,
}

enum CallableEntry {
    Script(FunctionRef),
    Native(NativeImportId),
}

enum CallTarget {
    Static(CallableId),
    Value { value: MirValue, signature: SignatureId },
    Interface {
        receiver: MirValue,
        member: InterfaceMemberRef,
        signature: SignatureId,
    },
}

struct NativeImport {
    binding: NativeBindingKey,
    contract_version: u32,
    instance: ConcreteFunctionIdentity,
    signature: SignatureId,
    required_callables: Vec<ResolvedCallableRequirement>,
}
```

Concrete signatures retain full parameter/result ABI types, access, nominal
identity and Never. Derive and check physical representations from these facts;
`HeapObject` alone cannot validate a call. Executable applications contain no
inference variables, generic parameters or unresolved projections. Generic
declaration templates may remain in module ABI for independent verification.

Resolved requirements retain contract requirement IDs, applied trait/member facts,
concrete callable targets and signatures. Script/native targets use the callable
table. Virtual targets retain the applied interface/member; invocation supplies
the receiver as a validated argument. Serialized requirements carry no per-call
values, HIR arena IDs or Rust addresses. Function-valued arguments are runtime
values, distinct from statically resolved requirements.

Generic MIR/bytecode verification checks table references, signatures, operands,
results and proof relationships. Trusted provider validation checks the complete
import application. Runtime linking checks actual handlers, authority and installed
generations, resolving each import once. Structural verification cannot authorize
native execution; decoded claims or cached seals cannot bypass provider-dependent
checks. Effects come from the trusted instantiated contract and conservatively
include callback/reentry effects. Bytecode and codegen consume the same handoff.

Arithmetic, indexing, fields, enums, storage and GC retain reviewed language
primitives. Public native methods always use the common import path, even when
their Rust implementations invoke these primitives internally.

#### Shared runtime lifecycle

Registrations supply a direct entry or an invocation factory. Factories produce
erased provider-owned state. Generic drivers handle bounded progress, a checked
call request, completion and failure without enumerating Array/Iterator/Option/
Result/Sort states. There are no speculative async wait variants.

Ordinary callback failures are explicit outcomes. Cancellation, exhaustion and
quarantine remain terminal driver state and cannot be recovered by a provider.
Context-managed roots, checked callable handles and guards pin values and provider/
dependency generations. No heap/table borrow or host lease crosses a callback.
Cleanup releases the native suffix without executing script. Preserve current
logical charges and failure positions until the separate execution-policy track.

#### Persistent native state and lazy iterators

Invocation state lasts until one call completes; iterator state remains reachable
between calls. The common invocation factory alone does not express that second
lifetime. Provide a runtime-managed, traceable native state cell and checked method
entries. This is one reusable state facility, not one GC or driver variant per
iterator adapter. Its concrete representation and access API are NR00 decisions.

```rust
struct NativeStateCell {
    state_contract: InstalledStateContractId,
    values: TracedValues,
    payload: OwnedNativePayload,
    callables: PinnedCallableSet,
    owner: PinnedProviderGeneration,
}
```

`TracedValues` holds all retained script values, including source iterators,
closure captures, intermediate items and a current flat-map inner iterator. These
are GC graph edges while idle, rather than permanent independent roots. Permanent
roots can keep unreachable cycles alive. Active invocations root their iterator
and temporary values; state updates pass through the heap's validation/barrier
interface. The opaque Rust payload contains owned algorithm data such as counters
and phases, without untraced script handles or escaped host borrows. Its storage
and tracing work remain subject to resource limits. Host domain state continues
to live outside the script heap behind existing typed handles.

`PinnedCallableSet` retains entry metadata and dependency owners. Callable value
captures reference traced slots rather than owning permanent roots independently
of the cell's reachability.

The installed state contract bounds the state representation and its checked entry
signatures; artifacts cannot manufacture state-layout or execution authority.
`Iter<T>` may retain its core representation and carry a native state cell plus
a pinned checked stepping entry returning `Option<T>`. Ordinary script-defined
Iterator implementations continue through their selected methods. Constructors
bind concrete output types and entries through the common native context. Runtime
checks the produced iterator's item type and owner, and every stepping result.
HIR sees the declared iterator type and callable application; MIR sees ordinary
constructor/next calls. Neither layer needs a Map/Filter/FlatMap target variant.

For `map`, construction retains the source and callback without advancing the
source or invoking the callback. Each `next` creates a bounded invocation that
requests source `next`, requests the callback for a yielded item, then returns
`Some(mapped)` or `None`. Each call releases temporary invocation state while
preserving the managed captures and source cursor for later calls. Aliases share
the same iterator state. Provider entries and selected source/callback dependency
generations remain pinned after the constructor frame returns and across reload.

`filter` can request several source/callback pairs during one `next`; every pair
must consume the existing logical work and poll cancellation. Returning no match
must not create an unbudgeted loop. `flat_map` additionally retains its active
inner iterator in the state cell, replacing that traced edge when moving to the
next outer element. Do not infer fused exhaustion for an arbitrary source;
adapter-specific exhaustion policy remains in its implementation and specification.

State access releases heap/table/Rust borrows before callbacks. Managed invocation
ownership and the existing guard contracts govern alias/reentrant access; define
and test conflicting active accesses before freezing this API. Idle cells cannot
retain execution frames, an active session or scoped host leases. Iteration guards
remain execution-scoped: early closure and terminal failure release the appropriate
dependency tree, and later resumption validates revisions and reacquires guards.
Cleanup performs no script callbacks or business finalization. No async wait or
cross-thread execution is introduced by persistent iterator state.

Extend the initial slice with `Iterator::map`: return it from a script factory,
invoke `next` repeatedly through aliases, force GC and reload between calls, and
verify pinned callback behavior and unreachable-state collection. Preserve existing
filter work-limit, non-fused, flat-map inner-state and early-close tests during
restoration. A frame-only callback test cannot establish persistent-state support.

#### Iterator::map implementation sketch

The declaration remains `fn map<U>(self, callback: fn(Self::Item) -> U) -> Iter<U>`.
The contract has one required callable, the applied source `Iterator::next`, and
one managed state product. Its state fields contain the source and callback; its
stepping result is `Option<U>`. The callback is a captured runtime value rather
than a statically selected witness. Contract instantiation validates all of these
relationships without a compiler/verifier branch for map.

The proposed provider API can make this registration explicit:

```rust
provider.register(
    MAP_CONTRACT,
    NativeEntry::direct(map_construct)
        .state_factory(MAP_STATE_CONTRACT, MapNext::start),
)?;
```

These are API sketches, not existing compiling repository APIs. Registration
resolves the state factory and its provider owner once. The constructor's context
receives that linked state product together with its instantiated signatures and
required callables; it does not search a global map-method registry.

```rust
const SOURCE: StateValueSlot = StateValueSlot::new(0);
const TRANSFORM: StateValueSlot = StateValueSlot::new(1);
const SOURCE_NEXT: StateCallableSlot = StateCallableSlot::new(0);
const REQUIRED_SOURCE_NEXT: RequirementId = RequirementId::new(0);

fn map_construct(
    cx: &mut NativeContext,
    args: NativeArguments,
) -> Result<RootedValue, NativeError> {
    let source = args.value(0)?;
    let transform = args.value(1)?;
    let next = cx.required_callable(REQUIRED_SOURCE_NEXT)?;
    let state = cx.new_registered_state(
        [source, transform],
        [next],
        (), // Map needs no persistent Rust counter or phase.
    )?;
    cx.new_result_iterator(state)
}

enum MapPhase {
    Start,
    WaitingSource,
    WaitingTransform,
    Finished,
}

struct MapNext {
    state: RootedNativeState,
    phase: MapPhase,
}

impl MapNext {
    fn start(state: RootedNativeState) -> Box<dyn NativeInvocation> {
        Box::new(Self { state, phase: MapPhase::Start })
    }
}

impl NativeInvocation for MapNext {
    fn resume(
        &mut self,
        cx: &mut NativeContext,
        event: NativeEvent,
    ) -> Result<NativeStep, NativeError> {
        match self.phase {
            MapPhase::Start => {
                event.expect_start()?;
                let source = cx.state_value(&self.state, SOURCE)?;
                let next = cx.state_required_callable(&self.state, SOURCE_NEXT)?;
                self.phase = MapPhase::WaitingSource;
                Ok(NativeStep::Call { target: next, args: vec![source] })
            }
            MapPhase::WaitingSource => {
                let value = event.returned()?; // Propagate ordinary call failure.
                match cx.read_option(value)? {
                    None => {
                        cx.end_iteration(&self.state)?;
                        self.phase = MapPhase::Finished;
                        Ok(NativeStep::Return(cx.result_none()?))
                    }
                    Some(item) => {
                        let transform = cx.state_callable_value(&self.state, TRANSFORM)?;
                        self.phase = MapPhase::WaitingTransform;
                        Ok(NativeStep::Call { target: transform, args: vec![item] })
                    }
                }
            }
            MapPhase::WaitingTransform => {
                let mapped = event.returned()?;
                self.phase = MapPhase::Finished;
                Ok(NativeStep::Return(cx.result_some(mapped)?))
            }
            MapPhase::Finished => Err(NativeError::InvalidContinuation),
        }
    }
}
```

Slot IDs above are private constants checked against this provider's state/requirement
descriptor, not global method IDs. The descriptor relates SOURCE to the first
parameter and iterator-guard dependency, TRANSFORM to the checked function parameter,
and SOURCE_NEXT to REQUIRED_SOURCE_NEXT's selected callable. Import requirement
identity and state-local slot index remain distinct. `new_registered_state` converts
temporary roots to managed graph edges, validates their concrete types and retains
callable metadata with value captures in traced slots. `new_result_iterator` binds
the registered stepping entry and the declared result's concrete item type.

Every next call gets a fresh `MapNext`; its root keeps persistent state reachable.
`Finished` marks only that invocation, not permanent source exhaustion. A later
next can observe Some after a previous None when allowed by the source contract.
State accessors return rooted values/checked handles and release internal borrows
before returning Call. `end_iteration` performs managed guard cleanup under the
existing dependency/alias policy, without executing a user close method.

The driver executes either source next or transform through the same checked
Script/Native/interface entry machinery and returns an owned, rooted outcome.
It contains no MapPhase switch. If transform fails, the source has already advanced;
that advance and completed side effects remain. Generic failure cleanup releases
the invocation and appropriate guards. Sticky termination bypasses recoverable
callback outcomes.

This sketch intentionally groups algorithm phases. The implementation must retain
the established logical charging, cancellation polls and allocation-failure points,
including Option payload extraction/construction. A single charge per shown phase
is not a replacement accounting policy. NR00 records the concrete managed-context
operations before implementing this example.

#### Minimal slice and removal boundary

Use a small `std::array` slice: `len` tests a direct generic receiver; `from_fn`
tests generic results and rooted function arguments; `sort_by` tests callback
resumption and prepared mutation; `sort` tests a selected `Ord::cmp` requirement.
Complete the first direct/callback/trait paths across NR01-NR03 as one reviewable
vertical outcome. A direct-only function is insufficient to freeze the data model.
Include an external direct function and a concrete host callback function that
invokes its supplied callable twice, without modifying generic core logic.

Before bulk removal, record the bounded public-route removal list and retained
language/storage/resource primitives. Revision `b42c35a` retains the previous
implementation; do not maintain a second compatibility implementation. Preserve
declaration sources as migration inputs and keep meaningful behavioral tests.
The installed package exposes checked declarations/contracts; missing handlers
fail executable linking, and unknown/malformed bindings still fail validation.
Do not substitute successful stubs or silently ignore invalid imports.

The user permits replacing old standard implementations before restoring full
coverage. Record intermediate build/test failures and their restoration owners
here. Do not remove or weaken tests to obtain a passing minimal build. NR04 restores
every predecessor public capability. NR05 closes carried ST06 obligations as well
as extension/behavior acceptance; no reduced-slice checkpoint claims full acceptance.

### NR00 — Replacement inventory and baseline

- [ ] Record the ST interim commit, passing evidence and open ST06 obligations;
  final combined acceptance is carried to NR05 rather than required at entry.
- [ ] Re-audit method-specific branches, contract duplication, provider asymmetries
  and primitive exceptions. Map each finding to NR01-NR05 in this ledger.
- [ ] Select descriptor ownership, trust/seal boundaries and the public callback
  representation without creating dependency cycles.
- [ ] Run applicable baseline checks and representative direct/callback workloads; record
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
- [ ] Carry the first-slice callback and trait-call requirements through the data
  model; complete their invocation path with NR02/NR03 before accepting the
  initial vertical replacement.
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
- [ ] Provide managed traceable state that survives calls; verify a returned lazy
  iterator through repeated steps, aliasing, GC and pinned generations without
  retaining an execution frame or scoped host borrow.
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
- [ ] Close carried ST06 final combined acceptance and matched measurements,
  distinguishing coverage restoration from generic-provider extension proof.
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
| Collections/lazy state | Preserve stable ordering, callback counts, alias visibility, structural guards, commit guarantees and early iterator cleanup; prove returned-state tracing/collection, bounded filter traversal and pinned stepping across calls |
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

NR00-NR05 are ordered replacement phases under the revised entry policy. Complete vertical
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

- 2026-10-01: Added a concrete proposed `Iterator::map` registration, constructor
  and per-next invocation sketch. Construction captures without traversal;
  source-next and transform are checked calls on the same driver. Distinguished
  captured callable values from required-callable metadata, persistent GC edges
  from invocation roots, and one-call completion from fused exhaustion. Recorded
  source progress after callback failure, managed guard cleanup and the requirement
  to retain detailed existing work/allocation boundaries. The API is illustrative
  and not implemented in Rust by this documentation checkpoint. All 27 local
  links/heading anchors and diff checks pass; no Rust tests were run.
- 2026-10-01: Follow-up design review for lazy iterators separates one-call native
  invocation from persistent GC-reachable state. Added managed traced value edges,
  owned Rust payload limits, pinned callable/provider generations and checked
  stepping entries. Extended the initial proof with a returned `Iterator::map` and
  retained filter/flat-map/early-close coverage. Current `gc/lazy_iter.rs` already
  separates captures from temporary stepping state, but `native/lazy_iterators.rs`
  still selects adapters from `NativeDefaultMethod`; the replacement removes that
  identity dependency. State representation and reentrant access remain explicit
  NR00 design decisions. Checked all 27 local documentation links and the diff;
  no Rust tests were run. This is documentation only, not an implementation claim.
- 2026-10-01: At clean revision `b42c35a`, the user requested HIR/MIR data design
  first, removal of old standard implementation paths and a small standard-library
  integration before restoring the rest. Replaced the ST06 entry gate with carried
  final acceptance. Added declaration/application separation, contract-owned
  callable requirements, concrete MIR callable/import tables and the common
  invocation lifecycle. The current audit also covers compiler `expr/native_*`
  witness policy omitted from the original audit table. Selected the `std::array`
  direct/callback/trait slice and an external callback consumer as initial proof.
  No implementation or deletion occurred in this design checkpoint; all NR phase
  acceptance checkboxes remain open. Documentation validation checked 91 local
  links and 16 heading anchors across the four changed documents; whitespace and
  diff review passed. No Rust build or runtime test was required for this change.
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
