# Native Provider and Contract Refactor Plan

Status: the minimal array slice uses registration-owned declarations and ID-based native linking; no NR phase is accepted.
The 2026-10-01 user direction changes the previous ST06 prerequisite: design the
HIR/MIR native boundary first, replace the old standard implementation paths and
prove a small standard-library slice before restoring the remaining algorithms.
[Standard-library and HIR integration](stdlib-hir-refactor.md) remains an interim
migration checkpoint with final combined acceptance and matched measurements open.
Those obligations carry into NR final acceptance; they are not claimed complete.
The [roadmap](implementation-roadmap.md) records the revised ordering. The first
checkpoint replaced method override policy. Declaration identities, common imports and a
direct/trait/callback array proof now execute; required-callable metadata, persistent
state, arbitrary external source catalogs and full-library restoration remain open.

## Current design decision (2026-10-01)

The user's latest direction makes native registration definitions authoritative.
The built-in library is an optional native API package installed by Engine by
default; application-owned native APIs use the same declaration, installation,
linking and invocation path. Generic infrastructure does not classify a function
as standard or host to select its implementation.

A native module owns type/trait/function declarations and implementation binding
IDs. Trait implementations derive their method signatures from the registered
trait contract rather than repeating them. Ordinary HIR imports these records
directly; runtime registration validates and retains the same portable declaration
records with Rust handlers. Generated `.kgr` files provide syntax, documentation
and navigation coordinates for tooling. They are not authoritative compiler input
and never confer handler-installation authority. The independent generated
`stdlib-declarations.bin` is removed from the minimal route.

This supersedes the earlier source-owned native linking decision, not its boundary
checks. Keep trusted registration, artifact application matching, host-owned state,
scoped borrow validation, roots, callback cleanup, budgets and generation pinning.
Primitive representations and language protocols remain engine capabilities.
Existing synchronous host adapters retain their checks during the bounded migration.

### Owner imports and re-export whitelist

The next user direction removes every existing Rust re-export across the
workspace, including restricted visibility. Consumers import actual owners;
intentional future re-exports require one exact reviewed whitelist declaration.
This is a breaking Rust API path migration within NR01, without changing portable
ABI records or Kagari language re-exports. It does not activate NR04 restoration.

- [x] Remove existing re-export declarations and expose the required owner routes.
- [x] Migrate implementation, tests, examples, macro expansions and consumers.
- [x] Reject unlisted re-exports in every parsed file/scope, including library roots,
  `mod.rs`, custom roots and tests; keep the initial whitelist empty.
- [x] Complete owner/visibility review and record final validation and inherited errors.

### Typed Rust authoring implementation

The latest user instruction activates a bounded typed adapter implementation and
supersedes the declaration/factory DSL below. This checkpoint exports real Rust
functions, array wrappers and trait impls; full opaque host derivation stays queued.

- [x] Replace the function-like DSL with `#[native_module]` and Rust item markers.
- [x] Derive checked metadata and invocation adapters from Rust value/return types,
  including aliases, Option and open generic rooted value proxies.
- [x] Register ArrayList/List/MutableList from actual Rust declarations and impls.
- [x] Retain resumable typed from_fn callbacks and predecessor logical budget steps.
- [x] Add Engine builder package selection and migrate application tests/examples.
- [x] Validate typed value ownership, negative Rust contracts and all focused routes.
- [x] Complete structural review and record the checkpoint validation ledger.

### Superseded declarative authoring implementation

The user requested replacing the raw ABI assembly in `standard_library()` with a
readable registration entrypoint. This bounded NR01 follow-up owns authoring and
preserves existing executable contracts; it does not activate the queued typed
Rust interoperability work.

- [x] Add a Rust-token native declaration macro and common expansion support.
- [x] Keep trait signatures/docs with declarations and bind impl methods without
  repeating signatures; generate definition IDs and generic owner/position records.
- [x] Colocate inherent/free-function signatures with their Rust descriptor factories.
- [x] Reduce default-library selection to ordinary package composition.
- [x] Prove external application authoring and retain exact generated source/artifact
  agreement, runtime output validation, callbacks, GC and feature boundaries.

### Registration-owned minimal implementation

Task: implement one complete native registration path before broader restoration.
Context: NR01 declaration design and NR02 registration/lifecycle, retaining NR04/NR05
restoration and final acceptance obligations.

- [x] Add source-independent native module records and generated declaration text.
- [x] Import native records directly into ordinary HIR, retaining generated coordinates.
- [x] Pair a complete package with Rust factories and validate before publication.
- [x] Install the array package by default through Engine, with an explicit opt-out.
- [x] Declare List and MutableList; derive ArrayList implementations from trait methods.
- [x] Prove application-owned native registration, execution and tooling through the same API.
- [x] Remove the binary declaration payload and its source-emission generation step.
- [x] Validate source, encoded and source-free execution; missing/duplicate/forged contracts;
  readonly rejection, callback/trap cleanup, generated navigation and completion.

The initial public model supports array storage, free functions, required traits,
generic inherent/trait implementations and existing callback function types.
Associated declarations, method-local generics, nonempty script bounds, arbitrary
Rust/opaque derives and the full legacy host type/path adapter migration remain subsequent
NR/RI work. Remaining library declarations are temporarily retained by the existing
source package; the minimal array module comes only from native registration data.
The generated text may be parsed for CST/tool queries, but is never lowered to
establish native semantics. No interpreter/JIT execution algorithm is duplicated.

## Full-library restoration sequence (2026-10-01)

Task: restore the remaining public library through registration-owned native APIs.
Context: the typed array checkpoint and owner-import migration are complete;
NR00-NR05 still own contract completion, restoration and combined acceptance.
Expected behavior: Engine optionally installs ordinary native packages; application
packages use the same mechanism; HIR checks their complete registered signatures;
compiler, verifier and VM consume checked contracts without library-method policy.
Scope: existing library coverage and the common capabilities it actually requires.
Arbitrary Rust/opaque derives, Serde, async and the queued execution-policy changes
remain in their own plans. The user activated this sequence in goal mode on
2026-10-01. Implementation begins with the math package checkpoint; no complete
NR phase is accepted by that bounded result.

### Readiness and remaining prerequisites

Restoration can start in small vertical checkpoints. It cannot yet consist only
of adding Rust functions: current typed adapters cover a bounded array proof.

| Boundary | Current evidence | Required before dependent families |
| --- | --- | --- |
| Registration and installation | NativeApi pairs declarations with handlers; Engine builder supports default opt-out and application packages | Keep duplicate/missing binding rejection and exact contract matching as packages grow |
| Static checking and tooling | Minimal array records go directly to HIR; generated array.kgr supports tooling | Extend records to remaining types, enums, associated declarations, bounds, method generics, defaults and namespace/prelude bindings |
| Typed values | Scalars, String, unit, Option, checked generic proxies and NativeArray work | Checked tuple, script Result and other existing enum/storage conversions; Result values must remain distinct from NativeResult execution failures |
| Typed callbacks | Explicit zero-to-eight argument packs, typed resumed results and repeated/nested callbacks use the common driver | Complete checked selected trait-call targets and remaining value representations |
| Type authoring | native_type accepts only a single NativeArray field; generic authoring accepts T: NativeValue | Extend only the existing Map/Set/Iter representations and checked script constraints required by restoration; retain Rust signature/trait conformance |
| Returned state | Per-invocation continuations and roots are exercised | Traceable managed iterator state across invocations, alias guards, cleanup and generation retention |
| Integration | Minimal source, encoded, source-free and external consumer proofs pass at 2b212880 | Migrate old ABI/HIR/compiler/runtime/VM/SDK fixtures and restore their missing library dependencies; full workspace acceptance is still open |

The current .kgr source package still supplies non-array declarations. That is
migration debt, not the target compiler boundary. All restored declarations must
come from registrations; generated text serves tooling only. Remove the existing
source-package route after its last declaration consumer has migrated.

### Coverage baseline and checkpoint rules

Use the predecessor [ST00 inventory](stdlib-hir-refactor.md#st00-ownership-and-behavior-inventory)
as the completeness checklist: 316 function entries, 116 trait methods, 38 explicit
native impl blocks, 13 type constructors and seven enums. Its 281 receiver entries
are alternate syntax, not extra algorithms. These are historical counts, not the
number of currently installed functions or a requirement to turn every language
primitive into a native call. Record each entry as a registration-owned algorithm,
a checked required/default implementation, or a justified retained primitive.

Current specifications and accepted registration APIs determine semantics and
signatures. Reconcile stale predecessor documents explicitly; do not revive
superseded APIs just to reproduce inventory counts. Preserve meaningful tests,
checked widths, UTF-8 boundaries, readonly access, evaluation/callback order,
prepared mutation commits, completed side effects and logical charging. Permission,
budget and host borrow changes are outside this restoration scope.

Each checkpoint must deliver declaration -> HIR -> artifact -> runtime execution
plus generated tooling coordinates. Add the same capability in an independent
application package to prove extension locality. Repair affected fixture builders
using current contract types; keep malformed-input assertions and runtime checks.
No compatibility aliases, forwarding crates, second dispatcher or successful stub
may bridge an obsolete model. Wire versions change only when executable schemas
change; affected products and fixtures are rebuilt in the owning checkpoint.

### Ordered work within NR00-NR05

These are checkpoints within the existing phases, not a second phase ledger.
Complete the contract/lifecycle proof before accepting NR01-NR03 or bulk-restoring
families that depend on it. Each NR04 family may use several cohesive commits.

| Order / owner | Concrete work | Exit evidence |
| --- | --- | --- |
| 1 / NR00 + NR01 | Map the inventory and carried diagnostics to the checkpoints below. Restore std::math floor/ceil/sqrt as the first ordinary free-function package, alongside the existing array package. Record representative baseline workloads. | Both packages compose; generated signatures agree; source and source-free execution work; default opt-out and an application-defined function use the same path |
| 2 / NR01-NR03 | Complete checked generic bounds, associated outputs, selected callable requirements and default-entry metadata. Generalize typed callback arguments/results. Register the required Ord/Ordering declarations; restore ArrayList sort_by and sort through checked callbacks/witnesses. Add the managed-state proof described below. | Direct, supplied-callback, selected-trait and returned-state paths all work for built-in and application packages without method-specific core dispatch |
| 3 / NR04 direct families | Register remaining type/enum/protocol declarations and primitive implementation facts. Restore numeric helpers and parsing, math, non-lazy String helpers, direct Option/Result queries and ordinary debug operations. | Width/overflow/radix and UTF-8 tests pass; script Result/error enums round-trip; protocol hooks use checked facts; no handwritten source supplies restored signatures |
| 4 / NR04 array/list | Restore the full ArrayList, List and MutableList surface: storage/capacity, positional/equality queries, source construction/copy/extend, ranges, prepared retain/dedup/sorting and windows/chunks. | Generic and dynamic readonly/mutable views, custom script implementations, stable sorting, once-only key extraction, alias guards and trap-before-commit coverage pass |
| 5 / NR04 iterator/string state | Restore collection/String/range iteration, Iter.next, all lazy adapters and terminals whose destinations are already available. Restore String bytes/indices/split/splitn/whitespace/lines. | Creation stays lazy; repeated and interleaved next calls share progress; captures survive forced GC; early termination releases guards; traversal stays bounded |
| 6 / NR04 map/set | Restore LinkedHashMap/LinkedHashSet storage, custom Eq/Hash lookup, updates/factories, retention, construction, readonly snapshots and set relations/algebra. Restore Iterator.group_by here. | Insertion/collision order, duplicate-key behavior, callback laziness, readonly snapshots and prepared update/retention guarantees pass under aliases and forced GC |
| 7 / NR04 composition | Restore Option/Result combinators, flatten/transpose, fallible FromIterator, collect/partition, Sum/Product, String.parse, debug.assert_eq and all remaining conversion/default/protocol implementations. Close every deferred inventory row. | Short-circuiting and error provenance, nested destinations, user-defined Iterator/FromIterator/FromStr/Ord/Eq/Hash implementations and cross-provider callbacks pass |
| 8 / NR04 retirement | Finish registration-owned namespace/prelude metadata; generate all tooling views. Remove kagari-stdlib and its handwritten declaration-loading/index/policy routes, update workspace/dependency assertions and obsolete fixtures, and rebuild the full feature artifact. | No semantic consumer reads generated .kgr; no remaining dependency on kagari-stdlib or obsolete binding catalogs; library coverage and all affected targets build |
| 9 / NR05 | Run the complete extension, behavior, feature/backend, documentation/example and reload matrices, whole-workspace checks and matched measurements. Resolve every carried diagnostic. | All final checks below pass, ST06 obligations close, and restoration plus no-core-edit extension acceptance are both demonstrated |

The first checkpoint replaces the existing empty legacy math declaration owner
with its registered module during installation; it must not publish two competing
std::math modules. Refresh the stale authoring descriptions in the standard
library README and standard-declarations specification with the implemented typed
attribute API. Update affected specifications and documentation examples as each
later family migrates, rather than postponing those contracts until final cleanup.

Step 2 must include a small returned iterator implemented by an application native
package. Its creation returns managed state and its next entry obtains a fresh
invocation. Captures and selected targets are traced/pinned across calls, not
stored in an untraced Rust vector or a global table keyed by library method IDs.
Prove GC threshold one, interleaved aliases, partial consumption, termination,
reload and cleanup without retaining a frame or scoped borrow. This is the minimal
state capability needed for steps 4-6, not full arbitrary Rust interoperability.

Selected trait calls in step 2 carry compiler-checked concrete targets, type
substitutions and effects through portable contracts. Native Rust implementations
request those targets through the common driver. No linker may infer that a
binding named sort requires Ord or that a map binding requires Eq/Hash. Defaults
also carry explicit selected implementation identities; the same rules apply to
user-defined traits and application packages.

Step 3's protocol declarations include cmp/hash/fmt/ops/convert/iter and the
existing associated types, bounds and primitive implementations. Registration
metadata describes public declarations and checked primitive selections. Numeric,
indexing, enum, range and closure representation machinery remains owned by the
engine with its existing validation; public algorithm names never select an
instruction. Complete primitive/declaration ownership before deleting legacy
sources. Only Rust re-exports require the empty whitelist; generated Kagari
namespace/variant/prelude exports retain their specified language semantics.

Steps 5 and 7 deliberately split traversal from destination composition.
Iterator.group_by depends on the Map implementation in step 6. collect/partition,
fallible destinations and Sum/Product finish in step 7, including selected
FromIterator/Sum/Product calls; they must not be silently omitted from step 5's
coverage ledger. Generic map/set key calls, retention and List equality require
step 2's common callable capability, even when primitive keys have an existing
storage fast path.

### Active restoration inventory and execution checklist

The ST00 member sets remain the detailed reference; this table gives every group
an execution owner without rebuilding a central method catalog in production.

| Inventory group | Owning checkpoint | Current state |
| --- | --- | --- |
| math's eleven helpers | 1 and 3 | floor/ceil/sqrt restored in checkpoint 1; remaining eight pending |
| 175 numeric methods and thirteen FromStr impls | 3 | Pending |
| String ordinary helpers / parse / lazy traversal | 3 / 7 / 5 | Pending |
| Option/Result ordinary queries / combinators and FromIterator | 3 / 7 | Pending |
| ArrayList plus List/MutableList methods and impls | 2 and 4 | Bounded registration proof retained; sort witness proof and remaining surface pending |
| Iterator lazy/default traversal, collection/String/range Iterable and Iter.next | 5 | Pending managed-state and associated-output prerequisites from 2-3 |
| LinkedHashMap/LinkedHashSet, Map/Set capabilities, snapshots, relations, group_by | 6 | Pending common calls and traversal |
| collect/partition, destination impls, Sum/Product and conversion blankets | 7 | Pending selected destinations and complete composition |
| debug direct entries / assert_eq | 3 / 7 | Pending |
| cmp/hash/fmt/ops/convert/iter contracts, seven enums and thirteen type constructors | 2-3 | Required Ord/Ordering starts in 2; remaining registration and primitive ownership in 3 |
| Implicit scalar/operator/index/range/closure implementation facts | 3 and 7 | Classify validated engine primitives in 3; composed/blanket contracts finish in 7 |
| Namespace, variant exports, prelude and legacy source crate | 8 | Pending migration and retirement |
| Existing fixture constructors and full artifact | Affected checkpoints 1-3 / 8 | Minimal math/array artifact updated in 1; carried old-model targets remain NR04 debt |
| Combined extension, behavior and ST06 acceptance/measurements | 9 / NR05 | Pending |

- [x] 1: math package, composition proof, inventory and entry baseline.
- [ ] 2: common selected calls, callback packs and traceable returned state.
  - [x] Typed outer argument packs and checked resumed results; external repeated/nested proof.
  - [ ] Checked bounds/associated outputs and selected callable/default requirements.
  - [ ] Traceable returned iterator state and generation/alias/cleanup proof.
- [ ] 3: remaining declarations, primitive facts and direct families.
- [ ] 4: complete ArrayList/List/MutableList behavior.
- [ ] 5: Iterator and String/range traversal/state.
- [ ] 6: Map/Set and custom keys/snapshots/grouping.
- [ ] 7: composition, fallible destinations and remaining protocols.
- [ ] 8: legacy source/fixture retirement and generated tooling closure.
- [ ] 9: NR05 and carried ST06 final combined acceptance.

### Verification and carried-error ownership

For the first checkpoint, retain the currently passing authoring and native proof:

```text
cargo test -p kagari-native-macros --lib
cargo test -p kagari-embed --test native_registration --test native_provider_artifact --test native_provider_reset --test host_interfaces
uv run python scripts/check_native_authoring.py
uv run python scripts/check_features.py --native-proof
```

Extend this proof with the restored math package and its application counterpart.
For later checkpoints reuse the affected existing suites, without disabling their
assertions or requiring unrelated future families to pass prematurely:

| Checkpoint | Existing behavior suites and required boundary probes |
| --- | --- |
| Contracts/callback/state | Native registration/reset/artifact suites; malformed signatures/witnesses; callback traps, sticky cancellation/budget failures, GC, reentry and reload |
| Direct families | standard_traits, string_extensions, string_parsing, enum_payloads, result_option; checked numeric/parsing and error-origin coverage |
| Array/List | array_operations, list_queries, list_mutations, list_windows, prepared_collections, collection_access and collection_interfaces |
| Iterator/String state | lazy_iterators, iteration_traits, string_extensions; partial consumption, non-fused sources, bounded filtering and generation-pinned captures |
| Map/Set | prepared_collections, collection_access, collection_interfaces; custom/colliding keys, mutation guards, independent snapshots and set relations |
| Composition | enum_combinators, result_option, iteration_traits, string_parsing, standard_traits; selected destination calls and early error/None |
| Retirement/final acceptance | standard_declarations, native_preparation, native_artifacts, artifact_features, source/tooling snapshots, documentation examples and the complete feature/backend matrix |

The latest inherited failures are reproducible with cargo test --workspace and
cargo clippy --workspace --all-targets -- -D warnings. ABI/HIR tests still refer to
EngineNativeBinding, removed standard bindings and RuntimePrimitive variants;
compiler/runtime/VM/SDK tests also use old NativeCall/NativeWitness/NativeImport
fields, HostImportId or binding_version. The full feature fixture represents the
predecessor library. NR04 owns these failures: migrate affected model constructors
alongside steps 1-3, missing behavior alongside its family in steps 3-7, and full
fixture/dependency closure in step 8. Do not defer all fixture repairs to NR05.
Record each newly observed failure with its command, diagnostic and exact next
checkpoint; do not repeatedly run an unchanged known failure.

Run the repository structural review and diff checks at each implementation
checkpoint. NR05 must pass the complete [verification matrix](#verification-matrix)
and final checks, including the full check_features.py consumer set rather than
only --native-proof. Preserve the empty Rust re-export whitelist. A restored-family
checkpoint or a passing minimal proof is not full-library acceptance.

## Objective and acceptance boundary

Treat standard-library native functions as entries in the installed runtime registry.
Standard and host entries share declaration, binding, contract checking,
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
| [Compiler native applications](../crates/kagari-compiler/src/source/lower/expr/native_contracts.rs) and defaults (`../crates/kagari-compiler/src/source/lower/expr/native_defaults.rs`; removed during reset) | Method-specific branches choose source/destination, key, lazy and trait witness applications | Contract-owned callable requirements, checked HIR selections and generic bounded materialization |
| [Callable identities](../crates/kagari-abi/src/callable.rs) | Public native methods use central engine operation families | Provider-owned binding identities, separate from language primitives |
| Native contract checks (`../crates/kagari-abi/src/native_import/contract.rs`; removed during reset) and callback checks (`../crates/kagari-abi/src/native_import/contract/callbacks.rs`; removed during reset) | Per-method signature, callback and constraint branches | Generic instantiation and trusted registration-contract comparison |
| [Effects](../crates/kagari-abi/src/effects.rs) and [import resolution](../crates/kagari-abi/src/native_import.rs) | Central method lists classify mutation, allocation and resumability | Trusted descriptor facts and linked entry capabilities |
| Witness validation (`../crates/kagari-abi/src/native_import/linked/protocols.rs`; removed during reset) | Iterator/Ord/aggregation-specific method selection | Explicit required callable applications and generic witness verification |
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

### One declaration model, registered implementations

Keep declaration checking and implementation installation separate:

1. Native registration definitions provide signatures, generic bounds, docs and
   method policy. HIR imports these records directly into ordinary checked declarations.
   Generated `.kgr` is a tooling projection. Existing host adapters retain their
   declaration facts and checks during migration without executing handlers.
2. MIR/bytecode carry generic native imports: binding ID, source declaration and
   applied type arguments, checked signature and bounds. They carry no standard
   operation enum or independently authored native signature/effect/access template.
3. Runtime registration pairs checked declaration metadata with a Rust handler or
   invocation factory. Algorithm state and implementation validation live here.

Native definitions expose declaration records without executing handlers. The
minimal array package derives ArrayList's List/MutableList methods from registered
trait contracts. HIR imports records directly and source compilation must emit
identical native contracts. `regenerate_native_provider_artifact` updates only the
KBC test fixture and the generated tooling declaration view. Runtime receives
ordinary ABI records from native installation without HIR/syntax/source dependencies.
Full-package restoration and legacy host adapter migration remain NR04/NR05 work.

### Binding identity, linking and trust

Use existing `DefinitionId` identities. Installed native annotations name a binding
within the declaration module's package namespace. This spelling is opaque to HIR,
compiler, verifier and VM: it does not choose per-function behavior. Host declarations
use their existing durable function identity. Source declaration identity, binding
identity and the module-local native import slot remain distinct.

Registration is a trusted embedding action. Reject duplicate binding IDs and invalid
declarations. At loading, resolve each import by ID, compare its instantiated
signature/bounds with the installed checked declaration and pin the resulting entry.
Unknown IDs and consistently forged source signatures must fail before entry. Merely
matching an ID is insufficient to validate untrusted artifact claims. Existing package,
artifact/runtime ABI, host schema/authority and generation checks remain in force.

Execution uses the linked slot without searching source declarations or standard
method catalogs. Entries remain alive across callbacks and hot reload. Source-free
verification checks carried declarations; actual execution also requires the trusted
installed declaration and implementation. Artifact bytes cannot install handlers or
choose their own effect classification. Cache seals remain installation-sensitive.

### Common metadata consumed by generic layers

| Fact | Producer and use |
| --- | --- |
| Signature and concrete application | HIR supplies ordered types, generic substitution and bounds; lowering transports them; validators check them generically |
| Method policy | Trusted declaration installation supplies override/default facts; HIR never derives them from a function ID |
| Required callables | Contract declares receiver/type relationships and required trait member identities; HIR selects witnesses; portable imports carry concrete callable applications |
| Effects and access | Every native call uses conservative effects; declared types/interfaces and ordinary bridges enforce writeability without a native access table |
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

Export checked declaration data from the existing source pipeline for source-free
installation. Do not add a standard provider-contract crate or another authored
signature catalog. Keep built-in registration in its implementation owner.
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
    Native(DefinitionId),
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

`DefinitionId` names an installed entry; it is neither a runtime slot nor an
authority token. Declarations own signatures and runtime installation owns handlers. No standard function/default/protocol enum belongs in this implementation
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

#### ABI and checked native declarations

ABI owns ordinary declaration/signature data, substitutions, bounded codecs and
generic verification. Export `.kgr` declarations after HIR checking; do not author
another native signature/effect/access descriptor. Runtime registrations consume
these checked declarations and pair their binding IDs with executable handlers.
Host adapters retain their schema, passing and scoped borrow facts independently
of standard-library algorithms. Both use the common import and invocation path.

Future required-callable dependencies must use checked bounds, applied trait members
and ordinary callable selections. Their precise portable representation remains
NR02 work. Do not restore per-standard-function witness selectors under another name.

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
    binding: DefinitionId,
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
checks. Native effects use the common conservative classification, including callback/reentry effects. Bytecode and codegen consume the same handoff.

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
- [ ] Select checked declaration export, trust/seal boundaries and the public callback
  representation without creating dependency cycles.
- [ ] Run applicable baseline checks and representative direct/callback workloads; record
  toolchain, machine, profile, features, default parallelism, cache state and input.

Exit: every remaining coupling has an owner; the measured baseline and concrete
consumer examples define behavior to preserve.

### NR01 — Generic provider contracts and source-free linking

- [ ] Implement declaration-derived binding IDs, exported checked declarations and
  runtime registrations with duplicate/version/authority checks.
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

- [x] Replace the HIR native-default override list with checked declaration policy
  and carry it through portable ABI validation.
- [ ] Replace remaining default-entry recognition with explicit selected callable
  metadata, including shared entries for final script defaults.
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

- 2026-10-01: Implementation began from clean `e506c54`. The first bounded
  data-contract checkpoint separates `MethodPolicy` from `MethodDefault` and
  stores the native default as a provider-qualified HIR binding. The installed
  declarations explicitly mark the existing 20 fixed defaults; HIR no longer
  derives override permission from `NativeDefaultMethod`. Policy participates in
  aggregate contract reuse, compiler ABI lowering, substitution and source-free
  interface checking. The generic verifier rejects explicit replacement of a
  final declaration before projection deferral and rejects final requirements
  and policy on non-trait declarations. User attributes/URIs do not install policy.
  Portable method omission still uses the existing engine-default rule until
  selected default-callable metadata replaces it in NR02; this checkpoint does
  not authorize omitted host defaults. Shared final script default entries also
  belong to that follow-up. Binding installation, algorithm factories, witnesses
  and persistent lazy state remain on the old paths; no standard algorithms have
  been deleted and no NR phase exit is claimed. The new ABI field requires runtime
  ABI v134, KBC v111 and KMIR v9; the feature fixture was regenerated, without old
  format readers. Validation passed: `cargo test -p kagari-abi -p kagari-stdlib
  -p kagari-hir -p kagari-compiler --features kagari-compiler/source --lib`
  (47 ABI, 191 compiler, 417 HIR and 7 package tests); the focused policy-boundary
  test; all 7 `cargo test -p kagari-embed --test artifact_features` tests;
  `uv run python scripts/check_features.py` for all four standalone SDK routes;
  workspace/all-target Clippy with warnings denied; structure review/checker
  (842 files, no violations/exceptions); format, 83 local links/anchors and diff
  checks. Logs are under ignored `target/nr-*.log`. No build/test errors carry
  from this checkpoint. Whole-workspace runtime/JIT acceptance and matched ST06
  measurements remain pending NR05; these focused results do not close them.
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

### Reset execution checkpoint (2026-10-01)

The user authorized deleting the old standard implementation before rebuilding a
minimal library. The replacement proof uses ArrayList new/len/push/from_fn, including
generic instantiation and a real script callback. This historical reset task is narrower than
the NR02/NR03 phase exit matrix: sort, persistent map and external callbacks remain
their owning phase's acceptance work. All predecessor behavior tests remain tracked;
their unavailable API cases belong to NR04 restoration, not a reduced final matrix.
Provider-owned contracts replace closed public-method binding identities. Generic
offline validation establishes structural agreement only; runtime installation must
compare the complete trusted contract and pin the actual entry before execution.


Reset acceptance, distinct from NR phase acceptance:

- [x] Delete old standard implementations and public-operation routing.
- [x] Connect provider contracts and concrete applications across HIR/MIR/bytecode/runtime.
- [x] Reinstall a small standard library and prove source, encoded and source-free execution.

The reset starts from clean `89a4a57`. It removes closed public intrinsic,
integer/radix, protocol/default binding and continuation-family catalogs, compiler
witness selectors and runtime collection/lazy algorithms. Required type/protocol
sources remain; other public standard methods are temporarily unavailable.
Predecessor tests retain their assertions and remain restoration obligations.
A scan of every deleted Rust file confirms no inline test functions were deleted.
No compatibility aliases, old readers or success stubs replace deleted code.

Implemented ownership:

| Owner | Reset data and execution model |
| --- | --- |
| ABI | Provider/entry key, per-contract version, independent generic binder, portable signature, parameter access, effects and optional full host authority declaration |
| Offline standard provider | Populated kagari-stdlib-provider crate; only ABI/common dependencies, no parser/HIR/handlers |
| HIR | Installed source signatures and shared immutable descriptor facts; ordinary type checking selects concrete applications |
| MIR | Uniform Native target carries a checked concrete import; existing function/interface targets handle script and trait calls |
| Bytecode | One deduplicated native import table for provider entries and synchronous host adapters |
| Runtime | Trusted registration factory and erased invocation state; full-contract linking, pinned handler owner, explicit roots and checked callback/completion |

The proof owns ArrayList new/len/push/from_fn. List len reuses the same entry;
List get is an ordinary script method, with its Index supertrait retained.
ABI signature checking includes enclosing impl binders. Primitive receiver impls
use trait-owner coherence instead of the old all-native-method exemption. Read
access is provider metadata, not a standard-method allowlist. Map/Set required
Iterable Item contracts remain, without an obsolete fixed Iter output constraint.

The reviewed engine inventory retains numeric arithmetic, layouts/discriminants,
generic index/aggregate operations, language comparison/hash/format hooks,
StringPartsJoin and Assert. GC owns bounded allocation, checked storage operations,
mutation preparation/commit, iteration guards and custom-key storage APIs; their
visibility supports provider code without exposing Rust host references or bypassing
validation. The array algorithm/factory lives in runtime native/array.rs. Generic
invocation, HIR policy, ABI and VM contain no array-method selector. Native effects
remain conservative at optimization time; runtime compares descriptor effects too.

Invocation Rust state uses explicit rooted argument/scratch slots. Returning script
values stay rooted after frame pop and before receive() retains them. Callback
signatures, values and results are checked; receive() can request another callback.
Loaded modules pin registration owners across callbacks/reload. Duplicate keys are
rejected; replacing provider generations is not yet exposed. Synchronous host
adapters retain full host registration, permission, schema, passing and borrow
checks while sharing the same import slots and frame driver.

This is a concrete initial model, not the full target design. HIR shares descriptor
values rather than InstalledContractId arenas; MIR embeds a concrete import before
bytecode deduplication. Arbitrary required-callable applications, selected default
callable tables, persistent traced provider cells and function-valued offline host
schemas remain NR01-NR03 work. Nonempty native declaration bounds are currently
unsupported. No returned lazy iterator is claimed. NR04 owns remaining algorithms,
API/test construction and full-library fixture restoration. NR05 owns the isolated
extension matrix, full acceptance and matched measurements.

Wire products use runtime ABI v135, KBC v112, KMIR v10 and helper ABI v6. Closed
native-binding versioning is removed; standard provider contracts are individually
v1. The new native_provider.kbc fixture proves the reset without replacing the old
feature_artifact.kbc with reduced behavior; the old bytes must be regenerated after
NR04 restores their APIs.

Passing reset validation:

- `cargo check --workspace`; `cargo clippy --workspace --lib -- -D warnings`.
- `cargo test -p kagari-embed --test native_provider_reset`: 8 tests covering
  ordinary/generic calls, captured heap callbacks, zero-count behavior, readonly
  access, forged provider/version/effect rejection, traps, every budget boundary
  of a nested callback workload and an embedding-owned registered callback entry.
- `cargo test -p kagari-embed --test host_interfaces`: 8 existing tests for
  permissions, output contracts, nested host roots, associated outputs, parent
  bridges, GC, synchronous reentry, reload and trap cleanup.
- `cargo test -p kagari-embed --test native_provider_artifact`: 3 tests for repeated
  source-free execution/GC, exact source/fixture agreement and unsupported wire version rejection.
- `cargo test -p kagari-embed --no-default-features --test native_provider_artifact`:
  2 tests. Production `cargo tree -p kagari-embed --no-default-features --edges normal`
  includes offline descriptors but excludes compiler, source stdlib, HIR, syntax,
  MIR and codegen.
- The same fixture passes all four feature routes: defaults (3 artifact tests),
  source-only (3), native-only (2) and neither (2). Reproduction: add
  `--no-default-features --features source` or `--features native` to the focused
  artifact command above. These prove the reset fixture, not the NR05 full-library
  standalone consumer/JIT matrix.

Carried integration errors (reproduced, owning phase NR04):

- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  fail compiling preserved legacy tests: E0432 EngineNativeBinding/standard::bindings,
  E0599 removed Integer/ArrayListNew/StringLenChars primitive variants, and obsolete
  NativeImport fields/witness constructors. Compiler tests report 369 errors and
  HIR tests 17 errors in the all-target check. These are intentional removed-model
  consumers, not production library build errors; their contracts/behavior must be
  ported and restored, not disabled or weakened.
- The old full-library fixture remains KBC v111 and intentionally cannot load as
  v112. Its source also uses removed APIs; NR04 owns re-emission after restoration.

Structural review found no introduced production globs, repeated parent traversal,
handwritten include files or expanded compatibility facades. The syntax checker
passes 787 Rust files with zero violations/exceptions. Native descriptor payloads
are bounded on decoding and included in aggregate artifact record accounting.
Root-slot arithmetic rejects overflow; callback progress uses a boxed request
after removal of obsolete error variants. Normal module boundaries isolate the
array algorithm and integration-test helpers. Format/diff checks and 103 live local
file links plus 19 heading anchors pass; four deleted historical Rust references
now retain plain paths instead of broken links. CRLF conventions are retained.
The focused proof targets also pass Clippy with warnings denied. No structural
allowances were added. Logs are ignored target/nr-*.log. No NR phase exit or full
acceptance is claimed.


### Source-owned native linking checkpoint (2026-10-01)

Starting from clean `b574ac8`, the user requested revising the minimal library after
rejecting the separate provider crate and duplicate signature/access/effect tables.
This checkpoint supersedes the reset's descriptor ownership without expanding into
library restoration or queued execution-policy changes.

- Removed `kagari-stdlib-provider`, `NativeContract`, `NativeParameterAccess`, numeric
  provider/entry keys and HIR's second native signature validator. Installed markers
  now produce opaque module-qualified binding IDs; ordinary HIR owns signatures.
- Portable callable implementations use `Native(DefinitionId)` for both standard
  and host entries; Engine/Host variants are absent from executable implementation
  identities. HIR's host index is only an input catalog reference, lowered to the
  same ID representation.
- Native imports retain source instance, binding ID, applied signature/bounds and
  optional existing host adapter facts. MIR uses the existing common conservative
  native effects. Runtime resolves IDs once and pins registered factories.
- Runtime installation receives ordinary compiled `NativeDeclaration` records.
  The generator exports the array module's declarations into a 2,129-byte bundled
  payload and the current KBC fixture. The exact payload is compared with source
  compilation; it is not another hand-maintained standard API catalog.
- List readonly access works through the existing ordinary interface bridge; its
  underlying ArrayList method uses its checked concrete signature. Native Read/Write
  parameter weakening and inherent receiver upgrades are removed. No new receiver
  syntax, permission matrix or script-heap borrow checker is introduced.
- Existing host schema, permission, passing, borrow/reentry and output checks remain
  in the host adapter. Standard and host calls share imports, frame dispatch, roots,
  callbacks and cleanup. Public host callback declaration support and completely
  unified frontend registration remain NR02/NR03 work.
- Replaced the obsolete independent-signature-mirror test with a forged source ABI
  regression: a consistent usize-to-u64 rewrite passes structural verification but
  cannot link to the installed source-derived len declaration. Unknown namespaces,
  unknown entries and wrong installed targets also reject before entry.
- Runtime ABI v136, KBC v113 and KMIR v11 replace the reset formats; helper ABI v6
  remains. No compatibility aliases/readers are added. The full-library fixture
  and old operation-enum tests remain NR04 restoration obligations.

Validation passes:

- `cargo check --workspace` and `cargo clippy --workspace --lib -- -D warnings`.
- Focused Clippy for the reset/artifact/host-interface targets and declaration
  regeneration example, with warnings denied.
- 21 focused tests: ten reset tests, three default-feature artifact tests and eight
  existing host-interface tests. They retain callbacks, GC-on-allocation, nested
  budget exhaustion, trap cleanup and independently installed Rust handlers. Added
  ordinary List::get execution observing changes through a writable alias.
- Artifact tests on source-only (three), native-only (two) and neither-feature
  (two) configurations; default source+native is included above. Generated runtime
  declarations and artifact bytes match source emission. `cargo tree -e normal`
  confirms the minimal production route has no stdlib source, HIR, syntax, compiler,
  MIR or codegen dependency.
- Structure review, format, local documentation links and diff whitespace checks.
  The checker covers 784 Rust files with zero violations and zero exceptions.

The old full-workspace test and all-target Clippy failures remain the reset's NR04
debt: consumers still reference removed operation enums/fields and unavailable
standard APIs. Their assertions are retained; this checkpoint does not repeat the
unchanged failing commands or reduce final acceptance. No production build failures
are carried. Nonempty native bounds/witnesses, persistent lazy state, broader library
restoration and full-package generation remain their existing NR01-NR05 obligations.
No NR phase acceptance or performance result is claimed.


### Registration-owned minimal checkpoint (2026-10-01)

Starting from clean `d4872720`, the user requested a concrete plan and minimal
implementation of registration-owned APIs, with MutableList as the example.
This checkpoint implements the bounded checklist above; it does not accept an
NR phase or expand into the remaining standard algorithms.

Implementation boundaries:

- `kagari-abi::native_api::NativeModule` owns portable declaration records and
  documentation. It validates supported shapes without rendering text or using
  source analysis. `implement_trait` substitutes an existing trait contract and
  binds its required methods; validation rejects altered signatures and missing
  local supertrait implementations.
- `kagari-runtime::NativeApi::new` pairs records with a complete set of
  `NativeHandler` factories, checking duplicates and missing/unknown bindings
  before publication. Composition and installation stage the registry atomically
  and do not execute factories. Existing import signature checks and invocation
  output/GC/callback checks remain in force. Factories are not statically typed
  Rust adapters yet: automatic Rust signature extraction remains subsequent work.
- The array package authoritatively registers ArrayList storage, new/len/push/
  from_fn, List len/get and MutableList set. ArrayList's trait method signatures
  come from `implement_trait`; runtime handlers provide the algorithms. List's
  Index dependency and Option/prelude metadata temporarily retain their legacy
  source path. Removing that remainder belongs to NR04 restoration.
- HIR imports records directly through ordinary declaration/type checking.
  Generated `.kgr` text provides CST, documentation and coordinates only; it is
  never lowered to establish native declarations. `AnalysisDatabase::default`
  and `Runtime::new` are bare; embedding/tooling owners install packages explicitly.
  Engine installs the default package unless `install_standard_library` is false.
  Low-level analysis/runtime fixtures must migrate to explicit installation during
  NR04 rather than restoring implicit standard-library policy in generic layers.
- Application native functions use the same NativeApi path. Generated declaration
  views are available through `native_declaration_sources()`; registered views are
  included in Engine analysis snapshots for definition, documentation and completion
  queries. Callers can persist these views as `.kgr` files. No standalone LSP server
  or external native catalog distribution protocol is introduced in this checkpoint.
- Deleted the bundled `stdlib-declarations.bin` and removed array source installation
  from the legacy manifest. `stdlib/array.kgr` is a tracked generated view.
  Regeneration updates that view and the existing KBC proof fixture. KBC v113,
  runtime ABI v136 and KMIR v11 remain because executable wire schemas are unchanged.

The registration flow is:

```rust
let api = NativeApi::new(vec![module], handlers)?;
let engine = KagariEngine::with_native_apis(
    EngineConfig {
        install_standard_library: false,
        ..Default::default()
    },
    vec![api],
)?;
let declarations = engine.native_declaration_sources();
```

See [the array registration](../crates/kagari-runtime/src/native/array_api.rs) for
MutableList's trait contract and derived ArrayList implementation,
[the application registration test](../crates/kagari-embed/tests/native_registration.rs)
for an independently authored Rust handler, and
[the runnable example](../crates/kagari-embed/examples/native_mutable_list.rs).
Run `cargo run -p kagari-embed --example native_mutable_list`: generic and dynamic
MutableList calls produce `I32(42)` and print the generated native declaration view.

Validation actually performed:

- `cargo check --workspace` and `cargo clippy --workspace --lib -- -D warnings` pass.
- `cargo test -p kagari-embed --test native_registration --test native_provider_artifact
  --test native_provider_reset --test host_interfaces`: 28 passing tests. Coverage
  includes registration errors, derived trait signatures, package opt-out, generic
  and dynamic calls, readonly rejection, encoded/source-free contracts, unknown/
  forged imports, callback budgets, GC, reentry and trap cleanup. Failed set retains
  the previous slot and releases roots; registration does not execute factories.
- Focused Clippy with warnings denied passes those four test targets and both native
  examples. The runnable MutableList example passes.
- `uv run python scripts/check_features.py --native-proof` passes standalone artifact-
  only, source-only, native-only and source+native consumers. All eight production
  dependency boundaries and the ABI normal/build graph exclude forbidden crates.
  The default full-library consumer mode remains unchanged and is still required
  for NR05; the new switch selects this bounded proof rather than bypassing it.
- `uv run --locked scripts/check_structure.py`: 791 Rust files, zero violations,
  zero exceptions. Format, local documentation link and diff whitespace checks pass.
  Manual structural review found no new production globs, repeated parent traversal,
  handwritten include files or unjustified re-exports. Direct HIR import, source
  rendering, package composition and array algorithms have separate module owners.

Carried errors and limitations (owning phase NR04; final NR05 acceptance stays open):

- Attempted `cargo test --workspace` and `cargo clippy --workspace --all-targets
  -- -D warnings`. Preserved old tests still fail to compile on removed
  `EngineNativeBinding`, `RuntimePrimitive::Integer/ArrayListNew/StringLenChars`
  and obsolete native import fields. These failures predate this checkpoint; the
  old assertions remain and need migration/restoration rather than compatibility
  aliases. Bare low-level analysis/runtime callers also need explicit package input.
- Attempted `cargo test -p kagari-stdlib --lib`: four pass and three fail. Two
  unchanged fixtures still expect the removed `#[intrinsic(...)]` marker model;
  the documentation check reports the existing undocumented map declaration.
  NR04 owns updating these fixtures and restoring declaration documentation.
- Full-library artifacts, all library behavior, associated declarations, nonempty
  native bounds, method-local generics, host opaque storage and the old host type/
  path adapter migration are outside this minimal checkpoint. No complete stdlib,
  whole-workspace acceptance or performance result is claimed.

Transient evidence lives under ignored `target/nr-registration-*.log`; reproduction
commands and durable scope/results are retained here. No structural allowances or
compatibility readers were added.


### Superseded declarative authoring checkpoint (2026-10-01)

Starting from clean `b97579c7`, the user requested improving the registration form
of `standard_library()`. This checkpoint replaces handwritten ABI assembly in the
array package with `native_module!`, preserving the previously accepted minimal
execution behavior and keeping broader NR acceptance open.

- Added `kagari-native-macros`, a proc-macro crate using syn/quote to read Rust
  authoring tokens at host build time. It has no Kagari syntax, HIR, compiler,
  ABI or runtime dependency. It emits construction calls into shared runtime
  authoring support; portable NativeModule records and NativeApi validation remain
  authoritative. No generated `.kgr` is parsed to establish registered semantics.
- The authoring block contains storage, required traits, inherent declarations,
  trait impl bindings and free functions. Documentation stays beside declarations.
  Trait impls bind names only and retain the existing signature-substitution and
  supertrait checks. Shared construction generates generic owners/positions and
  declaration IDs; authoring callers no longer assemble ABI structs or doc maps.
- `NativeFactory` separates execution descriptors (scratch roots plus entry closure)
  from script declaration identities. Entry factories still execute only through
  the existing invocation driver; descriptor construction does not invoke entries.
  Shared factory paths in one module install one binding, and conflicting paths
  with the same final name reject before publication. Binding names currently use
  the factory path's final segment; renaming that segment changes the binding ID
  and requires rebuilding consumers. Existing array binding IDs are unchanged.
- `standard_library()` only composes ordinary native packages. The array package
  declares List/MutableList and binds ArrayList methods in one readable block.
  Separate array factory functions preserve previous scratch counts, state steps,
  heap guards and callback/GC cleanup. No VM/HIR/linking algorithm is added.
- The application test and runnable MutableList example use the same macro.
  Example `demo::math::answer` supplies the second value while the default package
  supplies MutableList: execution remains `I32(42)`. Generated array text and the
  encoded fixture remain byte-for-byte equal; executable ABI versions are unchanged.

The implemented trait authoring form is:

```rust
trait MutableList<T>: List<T> {
    /// Replace a valid slot, trapping before mutation for an invalid index.
    fn set(self, index: usize, value: T);
}

impl<T> MutableList<T> for ArrayList<T> {
    set => array::array_set;
}
```

See [the complete array declaration](../crates/kagari-runtime/src/native/array_api.rs),
[default package selection](../crates/kagari-runtime/src/native/packages.rs) and
[the runnable external registration](../crates/kagari-embed/examples/native_mutable_list.rs).
The macro returns `Result<NativeApi, RuntimeError>`. Its default generated runtime
path is `::kagari_runtime`; `runtime = renamed_runtime;` supports renamed imports
and `runtime = crate;` supports authoring inside runtime itself.

Supported authoring forms match the minimal registration model: one-parameter
array storage, unbounded generic traits/impls/free functions, script `self`, scalar,
array-view, tuple, callback and standard enum types, and nominal supertrait bindings.
Unsupported Rust references, bounds, method-local generics, required method bodies
and non-documentation attributes receive compile-time diagnostics. Structural
contract errors still flow through NativeApi validation before package publication.
This is declaration/factory authoring, not automatic Rust signature extraction:
handlers remain low-level NativeContext/state-machine factories. Incorrect actual
returns still trap through invocation output checks. Typed Rust adapters, associated
declarations, full-library restoration and old host adapter migration remain pending.

Validation actually performed:

- `cargo check -p kagari-runtime` and `cargo clippy --workspace --lib -- -D warnings` pass.
- 34 focused tests pass: two proc-macro tests plus the native_registration (11),
  native_provider_artifact (3), native_provider_reset (10) and host_interfaces (8)
  integration targets. These retain ordinary/generic/dynamic calls, serialized
  fixture agreement, readonly rejection, callback budgets, GC, traps and reentry.
  Added declaration equivalence, duplicate generics, missing supertrait impls,
  malformed enum arity, factory-name collisions, consumer name/runtime-alias hygiene
  and incorrect output cleanup checks. Compile-time rejection tests cover unsupported
  references, bounds, required bodies and attributes.
- The MutableList example passes with `I32(42)` and prints default/application views.
  Focused Clippy for the macro crate's all-targets, those integration targets and
  both native examples passes with warnings denied.
- `uv run python scripts/check_features.py --native-proof` passes artifact-only,
  source-only, native-only and source+native standalone consumers and all production
  graph checks. The macro is a Rust build-time dependency and introduces no Kagari
  source-analysis dependency into source-free runtime execution.
- `uv run --locked scripts/check_structure.py`: 798 Rust files, zero violations,
  zero exceptions. Format, documentation link and diff checks pass. Manual review
  checked generated paths for hygiene: standard constructors use absolute paths,
  helper identifiers use mixed-site spans, and factory paths retain caller scope.
  The expansion support's public types are required by generated consumer code;
  they are not compatibility aliases or broad re-export facades. Source-token
  parsing, expansion, record construction and type-template construction have
  separate module owners, with no handwritten include files or new allowances.

The previous checkpoint's full-workspace/all-target and legacy stdlib fixture
failures are unchanged and remain NR04 restoration obligations. Those unchanged
commands were not repeated; no tests are disabled and no NR phase exit or whole-
workspace acceptance is claimed. Transient logs are `target/nr-authoring-*.log`;
these commands and results are durable resumption evidence.

### Typed Rust authoring checkpoint (2026-10-01)

The user requested implementing the final-facing API instead of continuing a
separate signature/factory DSL. This supersedes the preceding authoring checkpoint.
The bounded implementation is actual Rust functions, array storage wrappers and
required traits/impls, with one shared registration path for default/application APIs.

Implementation and ownership:

- `#[native_module("game::math")]` retains ordinary Rust definitions and consumes
  `#[native]`, `#[native_type]`, `#[native_trait]` and `#[native_impl]` markers.
  The proc macro generates adapters and a fallible `native_api()` entrypoint.
  Metadata comes from the actual NativeValue/NativeReturn types; type aliases
  resolve through Rust. Trait method bodies and signatures are checked by Rust,
  while portable records still undergo NativeApi validation before publication.
- Remove the old function-like macro/parser/expander directly. The proc-macro
  crate contains Rust item authoring and signature substitution only; it never
  loads a Kagari source parser. Shared expansion support still owns identities,
  declaration binders and record construction rather than a second validator.
- NativeCall owns roots and a pinned LoadedModule. Scalar/String/Option conversions
  are checked; GenericValue<slot> supplies one rooted erased representation per
  script generic position. NativeArray handles preserve heap identity, roots,
  ownership checks, shared storage, bounds and readonly mutation rejection.
  Cross-runtime transfers reject even when numeric heap IDs could coincide.
- Actual Rust ArrayList/List/MutableList contracts replace the array DSL and raw
  synchronous factories. NativeIndex bridges the existing script by-value Index
  contract without exporting unrestricted heap references. NativeFn<usize, T>
  drives from_fn callbacks through the predecessor's exact phase sequence and
  rooted driver; push retains its two logical steps. No generic HIR/VM dispatch
  code or executable ABI schema changes are needed.
- Engine builder collects fallible package registrations and validates composition
  on build; default installation remains optional. The Runnable MutableList example
  installs an ordinary Rust answer() -> i32 and returns I32(42).
- Distinct implementations cannot silently share a binding name. List::len has
  its own array_list_len entry rather than reusing the inherent Rust len wrapper.
  This changes a native binding identity: rebuild consumers using the old array
  declarations. Regenerate native_provider.kbc; generated array.kgr remains exact
  and unchanged because tooling declarations do not encode executable binding IDs.

The actual application entrypoint is:

```rust
#[native_module("game::math")]
mod math {
    #[native]
    pub fn is_positive(value: i32) -> bool { value > 0 }
}
let engine = KagariEngine::builder().install(math::native_api()).build()?;
```

See [the Rust array contracts](../crates/kagari-runtime/src/native/array_api.rs),
[typed value ownership](../crates/kagari-runtime/src/native_value/mod.rs),
[the runnable application](../crates/kagari-embed/examples/native_mutable_list.rs)
and [independent negative Rust checks](../scripts/check_native_authoring.py).
NativeResult<T> maps an error to a trap; NativeContinuation<T> uses the existing
resumable lifecycle with two scratch roots for this slice. NativeArray::new uses
its declared array result type. Supported generics require inline T: NativeValue;
arbitrary opaque Rust derives, method-local generics, associated declarations,
script bounds, full callback argument packs and the legacy host/path migration
remain later NR/RI work. Full standard-library restoration remains NR04-owned.

Validation actually performed:

- `cargo test -p kagari-native-macros --lib`: one unsupported-form test passes.
- `cargo test -p kagari-embed --test native_registration --test native_provider_artifact
  --test native_provider_reset --test host_interfaces`: 34 tests pass. Coverage
  includes typed aliases/Option/generic values under GC, retained array ownership,
  cross-heap rejection and cleanup, duplicate bindings, generic/dynamic MutableList,
  readonly reads/rejection, encoded fixture agreement, forged outputs/imports,
  nested callback budget boundaries, navigation/docs/completion and existing host
  permission/borrow/reentry/reload checks. No behavioral assertion was weakened.
- `uv run python scripts/check_native_authoring.py`: standalone consumers reject
  wrong Rust return types (E0308), mismatched Rust trait impl signatures (E0053)
  and unscoped &str parameters lacking NativeValue conversions (E0277). A valid
  consumer also compiles aliases, generic proxies, renamed runtime paths and
  consumer names that shadow standard constructors or adapter locals.
- `uv run python scripts/check_features.py --native-proof`: artifact-only,
  source-only, native-only and source+native standalone consumers pass, along
  with all eight production graph checks and source-independent ABI build checks.
- `cargo clippy --workspace --lib -- -D warnings`, proc-macro all-target Clippy,
  and focused embed targets/examples Clippy pass. `cargo run -p kagari-embed
  --example native_mutable_list` returns I32(42) and displays generated views.
- `uv run --locked scripts/check_structure.py`: 803 Rust files, zero violations,
  zero exceptions. Format, Markdown link existence and diff checks pass. Manual
  review covers macro token trees, explicit generated runtime paths, supported
  rooted representations, module ownership and re-export placement. The hidden
  AbiType re-export is confined to the intentional expansion boundary and avoids
  requiring every consumer to declare a separate ABI dependency.

The inherited full-workspace/all-target and legacy stdlib fixture failures recorded
above remain NR04 debt. Those unchanged failing commands were not repeated. This
checkpoint completes the requested bounded typed API; it does not accept an NR
phase or claim full Rust interoperability or full-library restoration.


## 2026-10-01 owner imports and empty re-export whitelist checkpoint

User direction: remove every existing Rust `pub use` and require explicit whitelist
approval for any future re-export. This supersedes the previous automatic facade
allowance, including the earlier hidden AbiType expansion facade. NR01 owns this
Rust API migration; NR04 still owns legacy native fixtures and library restoration.

- Remove all 172 actual Rust re-export declarations, including restricted
  visibility and the proc-macro re-export. Preserve Kagari `pub use` statements
  inside language fixtures: they test language semantics, not Rust API facades.
- Import each item from its actual owner throughout crates, examples, tests and
  standalone consumers. Open only the owner routes required by former public or
  crate-visible exports; retain member visibility, ownership checks and validation.
  No forwarding module, compatibility alias, second implementation or executable
  ABI change is introduced. Group imports by owner and preserve conditional scopes.
- Make ast_node a lexical parent macro defined before AST child modules instead
  of re-exporting it. Update identifier macros and native proc-macro expansions
  to name owners directly. Native attributes come from kagari-native-macros;
  generated wrapper conversion signatures name kagari-abi directly. The SDK's
  examples/tests and independent consumers declare those direct dependencies.
- Replace reexport-location with reexport-whitelist. Every parsed public/restricted
  use declaration produces a finding, including library roots, mod.rs, custom
  Cargo roots and tests. Only one exact file/declaration entry with review evidence
  can authorize it. The current structure-exceptions.toml has no entries. Preserve
  stale/duplicate/ambiguous-entry checks and independent wildcard enforcement.
- Review macro token trees, owner routes, visibility, aliases and conditional
  imports manually. The checker remains syntax-only and does not expand macros;
  current generated native adapters contain no re-exports. Update AGENTS.md,
  the structure policy and the embedding import/dependency guidance.

Validation actually performed:

- `uv run --locked scripts/check_structure.py --self-test`: 33 regression tests
  pass, including root/test whitelist authorization, location-independent denial,
  exact declaration matching, restricted visibility and stale policy rejection.
- `uv run --locked scripts/check_structure.py`: 803 Rust files, zero findings and
  zero whitelist/LOC exceptions. `cargo fmt --all -- --check`, changed-document
  Markdown link checks and `git diff --check` pass.
- `cargo clippy --workspace --lib -- -D warnings` and workspace binary Clippy
  pass. All-target Clippy passes for common, syntax, MIR, bytecode, codegen,
  Cranelift codegen, native macros and stdlib. Focused runtime host/layout/session
  targets also pass Clippy without warnings.
- Subsystem tests for common, syntax, MIR, bytecode, codegen, Cranelift codegen and
  native macros pass: 141 tests including doctests. The native_registration,
  native_provider_artifact, native_provider_reset and host_interfaces SDK targets
  pass all 34 tests. Runtime host_registration, host_nominal, struct_layouts,
  execution_sessions and runtime_substrate pass all 38 tests. No assertion or
  behavioral test was weakened.
- `uv run python scripts/check_native_authoring.py` passes all three negative Rust
  contracts and the positive alias/generic/renamed-runtime consumer.
  `uv run python scripts/check_features.py --native-proof` passes all four feature
  combinations, eight production dependency boundaries and the source-free ABI
  build boundary. `cargo check -p kagari-embed --test source_snapshots` passes.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  were attempted. They remain blocked by the already carried ABI/HIR fixture
  errors, including removed EngineNativeBinding, standard bindings and
  RuntimePrimitive variants. Broader compiler/runtime/VM/SDK target attempts also
  expose the carried old native call/witness/import fields, HostImportId and
  binding_version. Their owning follow-up remains NR04. Newly introduced missing
  owner imports and conditional-test imports were repaired; no alias was added
  to conceal those obsolete model errors.

This completes the requested re-export removal and enforcement checkpoint.
It is a breaking Rust import/dependency change and does not accept an NR phase
or claim full-workspace integration while the inherited fixture errors remain.

## 2026-10-01 full-library restoration planning checkpoint

User request: assess readiness and provide an execution plan for review.
Inspect the current native package selection, typed adapters, callback request,
legacy source manifest, ST00 inventory and existing NR phases at 2b212880. Record
remaining capabilities, the nine ordered checkpoints, per-family validation and
carried fixture owners in the proposed sequence above; link it from the roadmap.
This checkpoint changes documentation only. It does not restore an algorithm,
authorize the proposed implementation, accept an NR phase or close existing
whole-workspace failures.

Validation actually performed: repository structure check passes for 803 Rust
files with zero findings/exceptions; local Markdown link targets and the new
roadmap anchor are checked; existing line-ending conventions and git diff --check
pass. No build/test rerun is needed for this documentation-only proposal. The
prior implementation checkpoint remains the source of passing subsystem evidence
and inherited full-workspace diagnostics.

## 2026-10-01 restoration entry: registered math package checkpoint

User activation: start goal mode and execute the reviewed restoration sequence.
Checkpoint 1 advances NR00 inventory/baseline and NR01 ordinary package composition;
it does not accept either complete phase. The active inventory/checklist above
maps every ST00 group and carried fixture error to the remaining checkpoints.

- Restore floor/ceil/sqrt as actual Rust functions in native/math_api, selected by
  ordinary NativeApi composition alongside array. Derive signatures and adapters
  through native_module; preserve finite-value/domain checks and signed zero.
  No library identity is added to HIR, compiler, ABI verifier or VM dispatch.
- Remove math from the legacy source manifest. Its generated .kgr is tooling only;
  existing module-identity replacement imports the registered owner directly.
  Refresh authoring/spec/architecture descriptions and preserve the remaining
  source-package migration boundary.
- Regenerate the minimal source/encoded fixture with array plus math, including
  zero-argument math entries. Generate all installed library views with the same
  example. Check module composition, incorrect source signatures, registered-owner
  navigation/docs, optional installation and missing-handler linking.
- Exercise finite rounding, maximum finite sqrt, negative zero, NaN, both infinities
  and negative sqrt through newly encoded/decoded independently verified bytecode
  products under GC threshold one. Verify roots/depth cleanup and successful reuse
  after traps. SDK entry argument passing remains unsupported; boundary products
  supply constants through normal verified bytecode rather than changing the
  queued public call API or weakening validation.
- Extend the existing stdlib_baseline example with --native-proof workloads for
  direct array access, rooted from_fn callbacks and math. Full predecessor workloads
  remain required after restoration; this switch does not waive them.

Validation actually performed:

- Native math/registration/provider artifact/reset/host-interface suites: 38 tests
  pass; native macro unit suite: one test passes. No assertions were disabled.
- check_native_authoring.py: three invalid Rust forms rejected; alias/generic/
  renamed-runtime/hygiene consumer succeeds.
- check_features.py --native-proof: artifact-only, source, native and source+native
  consumers pass, including math boundary execution; eight production graph checks
  and source-independent ABI build graph pass.
- Workspace library Clippy and affected SDK targets/examples Clippy pass with
  -D warnings. Structure check: 805 Rust files, zero findings/exceptions. Format,
  local Markdown links, preserved line endings and diff checks pass.
- Full workspace/all-target checks are not repeated: unchanged obsolete fixtures
  remain owned by the checkpoints listed above. No wire schema changes occur:
  runtime ABI v136, KBC v113, KMIR v11 and helper ABI v6 remain current.

Entry measurement reproduction: cargo run -p kagari-embed --example stdlib_baseline
-- --native-proof. Rust/Cargo 1.98.1, rustc 48a229cea (2026-09-01), LLVM 22.1.8,
aarch64-apple-darwin; MacBookPro18,2, 10 physical/logical cores, 32 GiB RAM.
Use workspace dev profile opt-level 1, default source+native features, default
Cargo parallelism/target and warm build cache. Measurements exclude Cargo build
time, discard one warmup and report medians of 21 source compilations (including
Engine construction) and 101 interpreter executions of an already loaded program.
Each workload returns 42 and checks deterministic charges plus zero active roots.

| Workload | Compile median ns | Execution median ns | Logical steps | Artifact bytes |
| --- | --- | --- | --- | --- |
| native_direct | 112984125 | 15667 | 20 | 162818 |
| native_callback | 110442709 | 14958 | 48 | 152912 |
| native_math | 109826500 | 7709 | 23 | 153270 |

These are entry measurements after the first math registration, not a matched
predecessor comparison or an improvement claim. Record dispatch/allocation/memory
comparisons and the complete workloads at NR05. Next checkpoint: common typed
callback argument packs and result conversions, followed by selected trait calls
and managed returned-state proof. Goal remains active.

## 2026-10-01 common typed callback packs checkpoint

Checkpoint 2 advances NR03's public resumable authoring, but selected trait calls,
full declaration/bound support and managed returned-state proof remain incomplete.

- Replace the usize-only NativeFn request adapter with NativeArguments: an explicit
  outer tuple of zero through eight checked values. Convert arguments left to right
  without allocating a script tuple for the pack. NativeFn.result validates and
  converts resumed values through NativeValue; retain common closure/signature,
  heap, rooting and frame-driver checks.
- Migrate from_fn to NativeFn<(usize,), T> and request (index,). Its registered
  function type, IDs, executable schemas and predecessor logical phase sequence
  stay unchanged. This is a breaking Rust callback type/argument spelling change;
  update consumers directly, with no compatibility alias or re-export.
- An application-owned package implements zero-argument Option<String>, generic
  binary, one-argument unit and repeated unary callbacks using public owner APIs.
  Source -> encoded artifact -> runtime tests verify generic heap arguments/results,
  once-only observable effects, nested builtin from_fn calls, first-result survival
  across the second callback under GC threshold one, traps and complete cleanup.
- Extend independent native-proof consumers with the callback test target. Add a
  negative Rust consumer rejecting the obsolete scalar argument-pack type (E0277).
  Existing source-free array callbacks, default/application registration proofs and
  malformed artifact/output checks remain required; no new core dispatch case is
  introduced.

Validation actually performed: 40 SDK integration tests pass across native_callbacks,
native_math, native_registration, native_provider_artifact, native_provider_reset
and host_interfaces. Authoring checks reject four invalid Rust contracts and accept
the valid alias/generic/hygiene consumer. All four native-proof feature combinations,
eight production graphs and the source-free ABI build graph pass. Workspace library
and affected SDK Clippy pass with -D warnings; format, structure (807 Rust files,
zero findings/exceptions), Markdown links and diff checks pass. Macro token trees,
argument/value ownership, module visibility and empty re-export whitelist are
reviewed manually. Full-workspace legacy fixture debt remains unchanged in NR04.

Next: complete ordinary tuple/enum result conversions and checked associated/bound
metadata for the sort/selected-trait path, then implement the managed returned
iterator proof. The aggregate checkpoint 2 and all complete NR phases remain open;
goal mode continues.
