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
direct/trait/callback array proof now execute; typed callable/default metadata, persistent
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
| Static checking and tooling | Minimal array records, free-function/impl/inherent-method named trait bounds and ordinary associated declarations/projections go directly to HIR; generated views carry matching coordinates | Extend records to remaining types, enums, other declaration bounds, associated families, method generics, defaults and namespace/prelude bindings |
| Typed values | Scalars, String, unit, Option, one-to-eight member tuples, Ordering, rooted NativeResultValue, checked generic proxies, NativeArray, six immutable NativeRange shapes and generic Rust Bound payloads work | Remaining existing enum/storage conversions; Result values must remain distinct from NativeResult execution failures |
| Typed callbacks | Explicit zero-to-eight argument packs, typed resumed results and repeated/nested callbacks use the common driver; injected NativeSelected handles derive local/external and ordinary projected requirements; portable defaults resolve explicit template applications and execute through ordinary native slots | Associated families, method-generic selected authoring and remaining value representations |
| Type authoring | native_type accepts aliases and single-field tuple wrappers backed by actual NativeRepresentation adapters; generic authoring accepts T: NativeValue; enum shapes enter HIR directly with tooling coordinates | Extend the existing Map/Set/Iter representations and checked script constraints required by restoration; retain Rust signature/trait conformance |
| Dynamic interfaces | Ordinary application-native slots carry checked native import targets; real callable frames preserve output contracts, budgets, roots and implementation generations | Keep offline rejection, shared callbacks and retained versions as remaining declarations migrate |
| Returned state | Per-invocation continuations and roots are exercised | Traceable managed iterator state across invocations, alias guards, cleanup and generation retention |
| Integration | Minimal source, encoded, source-free and external consumer proofs pass at 2b212880 | Migrate old ABI/HIR/compiler/runtime/VM/SDK fixtures and restore their missing library dependencies; full workspace acceptance is still open |

The current .kgr source package still supplies declarations outside restored ops/array/math. That is
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
  - [x] Named free-function bounds, associated outputs and checked selected trait-member callback proof.
  - [x] Impl/inherent-method bounds, native method requirements and checked MIR target materialization.
  - [x] Typed local-trait selected handles, generated bounds and registration signature checks for free/inherent entries.
  - [x] External typed trait catalogs and foreign native implementation signatures, with actual provider installation and portable contract matching.
  - [x] Portable native default template applications, inherited obligations and encoded dynamic execution.
  - [x] Registered native default import and checked-source call/slot materialization.
  - [x] Typed Rust owned default authoring from real function templates and private helper identities.
  - [x] Retain and validate external default/template declaration dependencies before publication, installation and portable linking.
  - [x] Complete concrete default obligations against actual registered implementation facts.
  - [x] Ordinary projected selected receivers, implied base bounds and exact registered template retention.
  - [ ] Ord/Ordering and sort_by/sort.
  - [ ] Traceable returned iterator state and generation/alias/cleanup proof.
- [ ] 3: remaining declarations, primitive facts and direct families.
  - [x] Register the complete ops declaration surface and checked range/Bound representations; restore the array package's actual Index parent provider.
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
    NativeDefault(NativeDefaultApplication),
}

struct NativeDefaultApplication {
    declaration: DefinitionId,
    arguments: Vec<AbiType>,
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
Required awaits selection; Script and NativeDefault can supply defaults. An
explicit NativeDefault application selects an ordinary registered Native template;
it is resolved before executable imports or callbacks are published. Override policy
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
let api = NativeApi::new(vec![module], handlers, NativeCatalog::default())?;
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

## 2026-10-01 structural native values checkpoint

Checkpoint 2 advances the shared conversions needed by sorting and composition.
Complete bounds, selected callable requirements and managed state remain open.

- Add checked NativeValue implementations for one-to-eight member Rust tuples,
  including nested and single-member shapes. Validate the complete input shape
  and root all fields while decoding. Earlier output fields retain their roots
  while later conversions allocate. Keep value tuples separate from callback packs.
- Convert Rust Ordering to the existing checked script enum. Import zero-argument
  standard enum ABI types as named HIR types rather than invalid generic types.
  No comparison algorithm, method dispatch or executable schema changes occur.
- Add NativeResultValue<T, E> under its actual owner module. Preserve the rooted
  script Result object and original Err trace on round trips; decode selected
  payloads under applied ABI types. Fresh construction uses ordinary enum allocation
  and origin tracking. NativeResult remains execution failure, distinct from script
  Result branches. No automatic Rust Result copying is added, because it would
  discard the original error provenance.
- Application-owned source -> encoded -> runtime tests exercise nested generic
  tuple/Option/array callback values under GC threshold one, Ordering's three tags,
  Result construction/payload access, native traps, static shape rejection and exact
  original Err function/line retention. Active roots/call frames are released after
  success and traps, and explicit collection releases the objects.

Validation actually performed: 44 SDK integration tests pass across native_values,
native_callbacks, native_math, native_registration, native_provider_artifact,
native_provider_reset and host_interfaces. Workspace library and these SDK targets
pass Clippy with -D warnings. Format, structure (810 Rust files, zero findings or
exceptions) and diff checks pass. Tuple macro expansion ownership, API visibility
and the empty re-export whitelist are reviewed manually. All four independent
native-proof feature combinations, eight production boundaries and the source-free
ABI build graph pass. Full-workspace legacy
fixture debt remains in NR04, and wire versions/fixture bytes remain unchanged.

Next: declaration bounds/associated outputs and selected trait-call requirements
for sorting, followed by the application-owned managed iterator proof. Goal remains
active; aggregate checkpoint 2 and complete NR phase acceptance remain open.

## 2026-10-01 registered free-function bounds checkpoint

Checkpoint 2 advances NR01/NR02's ordinary constraint records; selected callable
dependencies, associated outputs and managed returned state remain incomplete.

- Accept existing named trait constraints on registered free-function ABI records,
  including generic arguments referring to the function's binder. Keep canonical
  ordering, exact owners/slots and declaration validation; reject unnamed legacy
  operation predicates in NativeModule records. Other declaration bounds and the
  typed macro's bounded NativeValue generic syntax remain unchanged.
- Render where clauses with target/constraint spans and import ABI constraints
  directly into ordinary HIR bounds. Fix zero-argument nominal traits to use named
  HIR types, matching their declaration arity. Generated text remains presentation.
- Carry concrete substituted requirements using the existing executable schema.
  Dependency-closure validation proves them before VerifiedProgram construction.
  Runtime registration retains bounded contract validation; linking consumes the
  sealed VerifiedProgram and requires complete installed/carried declaration
  equality, including bounds and passing metadata. Replace the empty runtime proof
  catalog, which could not prove nonempty bounds, without waiving signature, handle,
  permission, output or product validation. Apply the same check on reload paths.
- Add an independent application fixture with two generic parameters and both
  Container<T1> and Marker constraints. Source, encoded and source-free execution
  preserve a heap-backed Token through a native identity call under GC threshold
  one. Offline execution also works with default native installation disabled.
  Malformed binders/constraints, missing or wrong applied implementations, tampered
  product requirements and weaker/different installed templates are rejected;
  failed linking and static checks execute no native factory.
- Share the fixture registration between its integration test and source-only
  generator using justified test/cross-target path modules. Reproduce the checked
  product with cargo run -p kagari-embed --example regenerate_native_bounds_artifact;
  source emission must exactly match its encoded fixture. NativeDeclarationSite
  adds explicit bound coordinates, a breaking Rust record-shape change; executable
  wire versions remain runtime ABI v136, KBC v113, KMIR v11 and helper ABI v6.

Validation actually performed: 52 SDK tests pass across native_bounds,
native_values, native_callbacks, native_math, native_registration,
native_provider_artifact, native_provider_reset and host_interfaces. All four
independent native-proof feature consumers, eight production boundaries and the
source-free ABI graph pass. Workspace library and affected SDK Clippy pass with
-D warnings; format, structure (813 Rust files, zero findings/exceptions) and diff
checks pass. The unchanged native-provider fixture still matches source emission.
Review covers exact template authority, verified dependency proofs, reload linking,
module ownership, shared path justification and the empty re-export whitelist.
Full-workspace legacy fixture debt remains owned by NR04 and is not repeated here.

Next: associated declarations/output projections and checked selected callable
requirements, then sorting and the managed iterator proof. Free-function bounds
prove applicability; they do not identify or execute a required trait member.
No complete NR phase is accepted; goal mode remains active.


## 2026-10-01 ordinary associated-output checkpoint

Checkpoint 2 advances NR01/NR02's associated declaration and statically specialized
call contracts. General native interface slots, selected callable requirements,
default entries, sorting and managed returned state remain open.

- Accept ordinary registered associated declarations, named output bounds and
  qualified projections. Require exact associated owners and all impl bindings;
  derive native method signatures by substituting trait arguments/Self and resolving
  outputs. Reject missing/foreign bindings, unsupported associated families and
  mismatched method signatures. Existing portable contracts already carry these
  facts, so executable wire versions do not change.
- Render declarations, docs, bounds and impl-body bindings with corresponding
  coordinates. Import records directly as ordinary HIR associated members and
  projections; generated text remains a tooling view, never semantic input.
  Preserve checked associated member identities before output normalization and
  resolve them through the snapshot's declarations for navigation. Shorthand,
  qualified and explicitly bound output references navigate to the generated
  Item declaration and its docs, including when the output normalizes to i32.
- Extend actual Rust native traits/impls with type Item: NativeValue declarations
  and type Item = T bindings. Generate symbolic metadata for own associated slots
  and Self values, including nested Option/tuple forms and qualified own-trait
  projections. Implementation adapters use Rust's qualified concrete associated
  type; an invalid Rust signature still fails E0053 in the external authoring
  consumer. Symbolic metadata proxies delegate any value conversion to the existing
  checked/rooted generic proxy rather than exposing raw values or fake success.
  Associated families/defaults and inherited-slot authoring remain unsupported.
  NativeValue supplies representation; it is not exported as a script constraint.
- Correct generic ordinary trait calls selected from the aggregate catalog to use
  the common native implementation lowering path. Do not enqueue native methods
  as script bodies when collecting interface/parent/dependency demands. Static
  associated outputs normalize through the shared checked catalog.
- Remove registration's fake Unit instantiation of generic template layouts:
  valid projections cannot be normalized with invented arguments. Template binder
  validation remains mandatory, concrete declared layouts are still checked, and
  exact installed/carried template equality plus sealed product validation proves
  concrete import layouts and bounds before execution/reload linking.
- Add the independent game::associated/game::typed_associated fixture. An ordinary
  Source produces heap-backed associated values; a bounded free-function projection
  round-trips them. Actual Rust TypedSource and generic Transform impls exercise
  nested associated values and Self parameters/results. Source -> artifact bytes
  must match exactly; encoded execution under GC threshold one returns 42. Success
  and native error paths release roots/frames, explicit collection releases objects,
  malformed outputs/products fail offline, and bad script output bounds/type
  arguments fail statically without executing factories. The justified cross-target
  fixture module is shared by tests and its source-only generator.
  Both encoded entrypoints also execute with default native installation disabled.

Intermediate integration finding: application-native dynamic interface calls are
not complete. The associated fixture's earlier Source<Item = ArrayList<i32>>
coercion attempted to request a native head script body and failed with
Compilation/MirLowering MissingBinding("script body implementation"). The generic
static route is fixed; interface method products still only hold FunctionRef.
NR02 owns a common checked script/native callable target for dynamic slots and
selected native trait callbacks, including portable validation, generation pins
and the shared frame driver. Reusing the standard-specific native_bridge admission
or adding per-binding dispatch is not an acceptable repair. No dynamic application
interface acceptance is claimed, and the existing standard MutableList dynamic
behavior test remains enabled and passes.

Validation actually performed: the fixture generator succeeds and exact source
emission matches its bytes. 62 SDK tests pass across native_associated, native_bounds,
native_values, native_callbacks, native_math, native_registration,
native_provider_artifact, native_provider_reset and host_interfaces. Macro tests,
the external authoring proof (five invalid Rust contracts plus the valid hygiene
consumer), all four independent native-proof features, eight production boundaries
and the source-free ABI build graph pass. Workspace library and affected SDK
Clippy pass with -D warnings; format, structure (817 Rust files, zero findings or
exceptions) and diff checks pass. The added navigation reproduction failed before
the checked associated target fix and now passes for all three output forms.
Review covers template/product authority, checked navigation identities, macro
metadata versus actual Rust conformance, roots, generated visibility, justified
cross-target sharing and the empty re-export whitelist. NativeModuleBuilder's
macro support signatures and the public TypeTarget enum change Rust shape;
executable versions remain ABI v136, KBC v113, KMIR v11 and helper ABI v6.
Full-workspace legacy fixture debt remains NR04-owned. No complete NR phase is
accepted; goal mode remains active.

Next: replace script-only interface method slots with a common checked callable
target, then carry selected trait-member requirements into native imports and the
shared callback driver. Preserve malformed-product checks and generation-pinned
calls; restore sorting and the managed iterator proof after these capabilities.


## 2026-10-01 common native-interface checkpoint

Checkpoint 2 advances NR02's ordinary dynamic interfaces and shared callbacks.
Selected static trait-member requirements, default-entry authoring, sorting and
managed returned state remain open; no aggregate phase is accepted.

- Replace script-only InterfaceMethodSlot.function with the module-owned enum
  CallableTarget::Script(FunctionRef)/Native(NativeImportId). Lower ordinary
  native method applications from checked interface ABI records, deduplicate their
  imports locally and preserve deterministic declaration/argument table keys across
  dependency modules. Keep legacy host/native bridge handling while their owning
  routes are migrated; application methods do not acquire those exceptions.
- Verify each target kind, exact implementation/member identity, concrete arguments,
  binding and normalized trait signature. Require all nongeneric method slots on
  concrete tables, including native implementations. Reject missing/duplicate slots,
  absent imports, forged results and native-as-script targets before linking or
  factory entry. Generic templates still require concrete executable applications.
- Add real rooted native callable frames with pending/running/complete entry state.
  Charge entry/continuation steps at shared safepoints, account call depth and retain
  the implementation and interface receiver through return. Run trusted factories
  and transitions outside execution-frame borrows. Root callback returns during
  receive, validate outputs and use the existing shared completion/cleanup driver.
- NativeContext.interface_callback selects a method from an already checked rooted
  interface value. It retains the selected method generation and concrete result
  contract and drives script/native callbacks through the same stack. This proves
  dynamic selection; it does not yet carry a statically selected generic trait
  dependency in a native import. That remains the next NR02 responsibility.
- Error/debug frames expose CallableTarget instead of a fabricated FunctionRef.
  Native frames have their real declaration name/generation and no source span or
  script locals. Stepping remains over script safe points; traps and script
  callbacks can include retained native frames in stack snapshots. Migrate affected
  script-slot assertions/examples and host-bridge tamper builders to the explicit
  target model without weakening their behavioral checks. The compiler-only
  layouts example now explicitly installs an application-native array storage
  declaration instead of assuming that a plain AnalysisDatabase knows ArrayList.
- Extend the independent associated fixture with native/script dynamic interfaces,
  native-to-interface callbacks and a delayed native head continuation. Encoded
  execution works without default installation under GC threshold one. Allocation
  before retaining a callback's array output preserves it; success/trap/budget
  exhaustion releases frames/roots, and a too-shallow call budget enters no native
  factory. External direct invocation works and an old rooted interface continues
  to resolve its old implementation after reload while a new value selects the new
  generation. Native errors retain the head origin and script caller.
- Bump runtime ABI to v137 and KBC to v114 for the executable slot schema change.
  Rebuild native_provider/native_bounds/native_associated golden fixtures; exact
  source emission agrees. KMIR v11 and runtime-helper ABI v6 remain unchanged.
  Superseded full-library feature products remain NR04-owned rebuild work.

Validation actually performed: 67 SDK tests pass across native_associated,
native_bounds, native_values, native_callbacks, native_math, native_registration,
native_provider_artifact, native_provider_reset and host_interfaces. Workspace
library and affected SDK Clippy pass with -D warnings. The production library
check passes. Structural review covers module ownership, explicit owner imports,
empty re-export whitelist, bounded state transitions, callback return roots and
version retention. Format, structure (818 Rust files, zero findings/exceptions)
and diff checks pass. All four independent native-proof feature consumers, eight
production boundaries and the source-free ABI graph pass. Both compiler examples
applied_traits/layouts run successfully; their Clippy validation passes with
-D warnings. Full-workspace legacy fixture debt remains NR04-owned and is not
reported as accepted or disabled.

Next: carry checked selected trait-member dependencies through native declarations,
applications and linking, exposing them to the shared callback driver. Restore
Ord/Ordering and sort_by/sort after that proof, followed by managed iterator state.
Goal mode remains active.

### NR02 checkpoint: checked selected trait-member callbacks (2026-10-01)

Checkpoint 2 advances the static selected-call path for registered generic free
functions. It does not accept checkpoint 2 or any complete NR phase. Native
method/default requirement materialization, typed requirement authoring,
Ord/sorting, managed returned state and full restoration remain open.

- Add ordered NativeCallableRequirement records to native declarations. Each slot
  identifies a receiver, applied interface, member and member type arguments.
  Validate declaration ownership, generic binders, member arity and the declared
  receiver bound; preserve the records through direct HIR import. Generated text
  remains a tooling view and never determines callback target selection.
- Specialize requirements through the checked source catalog, normalize associated
  receivers/outputs and carry NativeCallableApplication records in native imports.
  Applications contain exact concrete target identity/arguments, implementation
  kind, parameter/result signature and common conservative effects. Demand private
  script bodies and native interface instances in the dependency closure.
- Reproduce selections with the source-independent ABI proof catalog. MIR/bytecode
  verification checks requirements against carried implementation facts and actual
  script/native targets; runtime linking still matches the complete trusted
  installed declaration. Bound checks alone do not authorize an arbitrary target.
  Selected effects use the shared native-call classification rather than claims.
- Add NativeContext.selected_callback(slot, arguments). The shared callback driver
  validates the complete argument list and result, roots values through receive,
  and retains the selected dependency generation. Selection creates no interface
  object or fabricated script body and performs no runtime trait inference.
- Prove application-owned selected_head<S: Source> against native Array receivers
  and a caller-private script implementation. nested_head additionally selects
  distinct S and S::Item receivers and invokes both native instances under GC
  threshold one. Source-free execution works with default installation disabled.
  Every logical budget cut reaches clean termination; callback traps and wrong
  slots/argument counts/types release frames/roots before target entry. Reloaded
  private script methods return 43 while old loaded programs retain 42.
- Reject forged/missing requirements, receivers, targets, type arguments, signatures,
  implementation kinds and effects; reject installed templates with removed slots.
  Bound ordering in the raw fixture follows the existing canonical ABI ordering.
- Bump runtime ABI to v138, KBC to v115 and KMIR to v12 for declaration/application
  schema changes. Helper ABI v6 remains. Regenerate native_provider, native_bounds
  and native_associated products; exact source emission and portable MIR/native
  preparation agree. Superseded full-library products remain NR04-owned work.

Validation actually performed: 74 SDK tests pass across native_associated (22),
native_bounds (8), native_values (4), native_callbacks (2), native_math (4),
native_registration (13), native_provider_artifact (3), native_provider_reset (10)
and host_interfaces (8). All four independent native-proof feature consumers,
eight production crate boundaries and the source-free ABI build graph pass.
Workspace library and focused SDK Clippy pass with -D warnings. Format, structure
(822 Rust files, zero findings/exceptions) and diff checks pass. Manual review
covers explicit owner imports, the empty re-export whitelist, requirement/target
ownership, decoding bounds, callback roots and retained generations. Full workspace
and all-target legacy fixture failures remain NR04-owned, were not rerun unchanged,
and are not reported as accepted or disabled. Old fixture builders must use the
new declaration/application fields when they migrate.

Next: materialize requirements for native methods and dynamic slots through checked
MIR facts, add typed requirement authoring, then restore Ord/Ordering and
sort_by/sort. Prove managed iterator state before bulk dependent restoration.
Goal mode remains active.


### NR02 checkpoint: checked native method applications (2026-10-01)

Checkpoint 2 now carries selected requirements for registered native methods,
including dynamic interface slots. No complete NR phase is accepted. Typed
requirement authoring, native defaults, Ord/sorting, managed returned state and
the remaining library restoration are still open.

- Materialize module-owned concrete native applications in MIR while the checked
  source catalog is available. Include complete signatures, inherited bounds and
  selected dependencies; demand their script/native instances and count generic
  instances in the existing whole-program budget. Share application traversal
  between instruction calls and body-free native targets.
- Require ordinary native interface slots to have corresponding checked targets
  before portable MIR can be sealed, including dynamic-only programs. Backend
  lowering copies those contracts instead of reconstructing an application from
  interface signatures. Portable linking reselects requirements and checks actual
  native targets as well as script bodies.
- Add implementation bounds and inherent-method bounds to registration records,
  HIR import and generated tooling coordinates. Canonically combine inherited
  obligations without silently accepting malformed original bounds. Trait method
  implementations cannot strengthen their contracts. Bound targets retain the
  specification's generic-parameter/projection restriction.
- Extend the independent application fixture with Array<T>: Source where T: Hook
  and an inherent check_first entry requiring T: Hook. Both invoke caller-private
  script methods through the shared driver, including forced allocation under GC
  threshold one, direct/dynamic calls, nested selected native calls, budget cuts,
  traps and generation-pinned reload. Root/frame counts return to zero.
- Reject missing, duplicate and forged portable native method targets, removed
  selected requirements, incorrect signatures/owners and malformed declaration
  bounds. Missing Hook implementations fail source checking before execution.
- Bump KMIR to v13 for the explicit native target table. Runtime ABI v138, KBC v115
  and helper ABI v6 stay unchanged: their schemas already contain complete native
  applications and portable MIR remains a separately versioned opaque payload.
  Regenerate the three focused KBC products and verify exact source emission.

Validation actually performed: the previous 77-test SDK run plus the added callback
trap test yield 78 passing SDK tests, including all 26 native_associated tests.
The four independent native-proof consumers, eight production boundaries and
source-free ABI graph pass; source-enabled associated consumers are rerun after
the additional trap test. Workspace library and focused SDK Clippy pass with
-D warnings. Format, structure (823 Rust files, zero findings/exceptions) and diff
checks pass. Manual review covers bounded decoding, explicit owner imports, the
empty re-export whitelist, checked target ownership and callback roots/version
retention. Existing full-workspace and all-target fixture failures remain owned by
NR04; unchanged failures were not rerun or disabled, and final acceptance is open.

Next: expose typed selected-call requirements in Rust authoring and complete native
default metadata, then restore Ord/Ordering and sort_by/sort. Prove managed iterator
state before dependent restoration. Goal mode remains active.


### NR02 checkpoint: typed selected dependency authoring (2026-10-01)

Checkpoint 2 exposes selected dependencies in the typed Rust authoring API. This
accepts only the bounded local-trait/free-or-inherent authoring proof, not the
complete checkpoint or any NR phase. Cross-module typed catalogs, projected
receivers, member-local generics, native defaults, sorting and managed returned
state remain open.

- Add injected `#[selected(T: Trait::method)]` parameters with
  `NativeSelected<A, R>` handles. Derive the generic script bound and exact member
  requirement from the annotation; exclude the injected parameter from the script
  signature. Preserve Rust body/type checking and generated tooling coordinates.
- Resolve typed packs and results against the registered trait signature at
  publication, including declared associated output bindings. Reject absent
  members, wrong packs/results, non-generic receiver targets and conflicting or
  malformed markers. Canonical bounds deduplicate repeated obligations while
  ordered requirements preserve distinct injected selections.
- Keep the checked application and original dependency generation in the typed
  handle. Convert requests/results through NativeArguments/NativeValue and reuse
  the existing selected callback driver. Do not select an unrelated slot from a
  later context or introduce another dispatcher.
- Generalize NativeResult's NativeReturn adapter to wrap a continuation as well
  as immediate values; retain the wrapped return's scratch requirement. A native
  execution failure remains distinct from a script Result value.
- Add an independent application fixture with associated-output Echo methods,
  generic native array implementations, private script implementations, a selected
  free entry and an inherent entry. Two ordered selections share one bound; the
  second receives a binary argument pack while the first heap result stays rooted.
  Exercise forced GC, default opt-out, encoded execution, factory/callback traps,
  every logical budget cut and pinned script generations after reload.
- Extend independent feature consumers and Rust compile-rejection checks with the
  typed selected proof. Keep ABI v138, KBC v115, KMIR v13 and helper ABI v6: this
  authoring change uses the already checked executable requirement schema. The new
  fixture has its own source-to-artifact generator; existing products remain exact.

Validation actually performed: 86 SDK tests pass across the ten focused targets,
including eight typed selected tests and 26 associated tests. All four standalone
native-proof feature consumers, eight production crate boundaries and the
source-free ABI build graph pass. The macro rejection suite passes; the separate
Rust consumer rejects six invalid contracts and compiles the alias/runtime-path
hygiene case. Workspace library and focused SDK/example Clippy pass with
-D warnings. Format, structure (829 Rust files, zero findings/exceptions) and diff
checks pass. Manual review covers generated token paths, explicit owner imports,
the empty re-export whitelist, hidden descriptor ownership, typed/checked signature
agreement, callback roots and generation retention. The new artifact matches
source emission exactly. Full-workspace/all-target obsolete fixture failures remain
NR04-owned; unchanged failures were not rerun or disabled. Final acceptance is open.

Next: provide the checked external trait catalog and native default contracts
needed by Ord/Ordering and sort_by/sort, then prove managed iterator state. Keep
NR04 integration failures and the full NR05/ST06 behavior/measurement obligations
in scope. Goal mode remains active.

### NR02 checkpoint: external native trait catalogs (2026-10-01)

Checkpoint 2 extends typed selected authoring and native impl registration across
package boundaries. Projected receivers, member-local generics, native defaults,
Ord/sorting and managed returned state remain open; this does not accept an NR
phase or complete the restoration goal.

- Add an immutable NativeCatalog declaration view, obtained from validated APIs.
  Explicit `#[native_module("game::consumer", catalog)]` generates one
  `native_api(&NativeCatalog)` entry. External selected annotations use exact
  fully qualified script identities. Retain the selected trait and its declared
  parent closure; missing parent declarations cannot acquire implicit authority.
  Identical shared views deduplicate; conflicting contracts reject composition.
- Map external Rust trait impls with an explicit
  `#[native_impl(contract = "game::provider::Echo")]`. Preserve actual Rust trait
  calls, arguments and associated types. Derive signatures from the authoritative
  TraitAbi and compare types against actual Rust method descriptors before API
  publication. Parameter names do not redefine the contract, and a fabricated
  catalog cannot authorize incompatible Rust methods.
- Make the low-level NativeModule.implement_trait take the explicit TraitAbi.
  Reuse its existing signature derivation to validate local and foreign impls.
  Standalone records defer unavailable foreign contracts to closed composition or
  installation, which requires actual owning providers. Generic applicability and
  parent witnesses retain ordinary HIR and portable proofs; this signature check
  does not introduce a second type resolver. Catalogs currently contain registered
  native declarations, not the remaining legacy primitive/source trait contracts.
- Stage owned trait records, required contract checks and handlers together during
  installation. Missing, changed or duplicate providers reject without partial
  publication. Retain package contracts with installed entries and compare them
  with the verified product's public trait declarations before executable linking.
  A well-formed, sealed product cannot silently replace a registered dependency.
- Establish ordinary private HIR imports for foreign record references. Generated
  declaration text stays a tooling view. Exercise external generic selections
  against script and native receivers, a foreign native bool impl, dynamic calls,
  inherent entries, forced GC, every budget cut and retained script generations.
  Array representation coherence still rejects overlapping wrapper impls; the
  proof uses distinct bool and array receiver shapes without waiving that rule.
- Regenerate only the selected fixture and preserve its exact source emission.
  ABI v138, KBC v115, KMIR v13 and helper ABI v6 remain unchanged because the
  executable contract schema is unchanged. All actual owners and call sites use
  the new raw and generated APIs; no compatibility alias or re-export is added.

Validation actually performed: 83 SDK tests pass across ten focused targets, including 13
selected tests and 26 associated tests. Macro option/rejection tests pass. The
separate Rust consumer rejects seven invalid contracts and compiles runtime-path,
alias and external trait-mapping hygiene. Workspace library and focused SDK/example
Clippy pass with -D warnings. Format, structure (830 Rust files, zero findings or
exceptions) and diff checks pass. All four standalone native-proof feature
consumers, eight production crate boundaries and the source-free ABI build graph
pass. Manual review
covers import ownership, generated paths, the empty re-export whitelist, immutable
catalog authority, staged installation, actual Rust/registered signature agreement,
public/private contract ownership, callback roots and retained generations.
Full-workspace/all-target obsolete fixture failures remain NR04-owned; unchanged
failures were not rerun or disabled. Final acceptance remains open.

Next: complete native default contracts and projected requirements needed by
Ord/Ordering and sort_by/sort, then prove managed iterator state. Preserve NR04
integration errors and NR05/ST06 combined behavior and measurement obligations.
Goal mode remains active.

### NR02 checkpoint: portable native default applications (2026-10-01)

Checkpoint 2 now validates native default declarations and executes encoded dynamic
default calls. This accepts the portable foundation only. Rust/source default
authoring and materialization, projected requirements, associated families,
Ord/sorting and managed returned state remain open; no NR phase is accepted.

- Add NativeDefaultApplication with an actual native template DefinitionId and
  explicit symbolic arguments. Keep declaration metadata distinct from execution:
  selected callbacks and executable imports require a resolved ordinary Native
  instance. Do not infer Self, associated outputs or template order from binding
  names, and do not synthesize a script body or add a second runtime dispatcher.
- Supply validated native declarations to the shared source-free ProofCatalog.
  Check template arity, parameter mutability, applied parameter/result types and
  obligations under the trait/method bounds and implicit Self contract. Generic
  assumptions entail declared parents through the bounded ancestry proof; ordinary
  associated outputs retain their canonical Self projections and declared bounds.
  Concrete selection checks normalized template arguments and concrete bounds.
- Apply default mappings during interface instantiation and selected resolution.
  Final methods require the canonical inherited application; changed template IDs,
  arguments, opaque Native replacements and Script replacements reject. Different
  members may share one checked physical native target; method identities remain
  unique. Dynamic slots check the full mapping, binding and signature, rather than
  incorrectly equating template arguments with the implementing table's arguments.
- Validate default slot targets in linked portable MIR/bytecode. A MIR native target
  can belong to another module; complete dependency proofs still authenticate its
  owning declaration and exact application. Runtime retains receiver/heap/version
  checks and uses the sealed target on the common driver. HIR/source default
  producers and source-to-MIR lowering are still pending, not accepted by this test.
- Add nine independent ABI tests, including reordered trait/Self parameters,
  associated output mapping, inherited template obligations, final override
  rejection, malformed signatures/bounds, bounded decode and cancellation. Add
  four encoded-product tests using actual registered Rust functions and selected
  callbacks with default installation disabled, GC threshold one, shared default
  targets, forged applications and every logical budget cut. Root/frame counts and
  allocated heap objects return to zero after resource failure.
- Bump runtime ABI to v139, KBC to v116 and KMIR to v14; helper ABI remains v6.
  Regenerate the four focused products and preserve exact source emission. Older
  products require rebuilding; do not patch their headers or add a legacy reader.

Validation actually performed: nine ABI integration tests and 87 focused SDK tests
across eleven targets pass, including four default tests, 13 selected tests and 26
associated tests. Workspace library and focused ABI/SDK Clippy pass with warnings
denied. Format, structure (833 Rust files, zero findings/exceptions) and diff checks
pass. All four standalone native-proof feature consumers, eight production
boundaries and the source-free ABI graph pass. Manual review covers actual owner
imports, the empty re-export whitelist, template binder/target ownership, bounded
decoding and proof work, canonical final applications, shared slots, callback roots
and retained generations.

Integration checks attempted and still NR04-owned:
`cargo test -p kagari-abi --lib` fails before execution on obsolete
EngineNativeBinding and RuntimePrimitive::Integer/ArrayListNew/StringLenChars
imports in old validation/numeric tests; 14 compiler errors include cascading slice
diagnostics. `cargo test -p kagari-embed --no-default-features --test artifact_features`
fails before execution on NativeImport.binding_version, which no longer exists.
Logs are target/native-default-abi-lib.log and
target/native-default-carried-full-artifact.log. These checks are neither accepted
nor disabled. Full workspace/all-target integration and the old complete artifact
remain open; all errors must be resolved before NR05/ST06 acceptance.

Next: derive default applications from actual typed Rust function templates,
import them into HIR and materialize checked source calls and slots without
implementation-owned fake bodies. Then complete projected requirements and
Ord/sorting, prove managed iterator state, restore dependent families, retire
legacy callers and run NR05/ST06 combined behavior and measurement acceptance.
Goal mode remains active.


### NR02 checkpoint: registered source default materialization (2026-10-01)

Checkpoint 2 now imports raw registered defaults into HIR and compiles their direct,
generic, dynamic and selected calls. Typed Rust attribute authoring, complete
external default-template dependencies, projected requirements, Ord/sorting and
managed returned state remain open. No NR phase is accepted.

- Retain explicit NativeDefault applications and MethodPolicy in imported records.
  Do not lower generated declaration text to determine whether a method has a
  default. Apply checked Self/trait/associated-output substitutions to ordinary
  native templates and retain full signature and selected-dependency validation.
- Native implementations bind only required members and explicit overrides.
  Omitted inherited defaults keep their canonical template application; final
  bindings and script overrides reject. HIR implementation identities can select
  that template without inventing an implementation-owned script body.
- Materialize actual template imports and native selected targets in their owning
  modules. A foreign target retained for a local interface slot does not satisfy
  the owner's executable callback demand. Portable bytecode uses the shared
  source-free proof catalog to normalize the carried mapping, then copies the
  corresponding already checked MIR native target; it does not resolve syntax.
- Validate registered default templates before API publication, composition and
  staged installation. The current raw proof covers symbolic obligations under
  declared contracts. Literal concrete bound obligations need implementation
  evidence, and standalone external default catalogs need native-template closure;
  these are follow-ups in typed authoring, not waived validation or accepted APIs.
- Prove source-to-encoded direct/generic/dynamic/selected calls for script and
  native bool receivers, explicit overridable script methods and final rejection.
  Generic interfaces return script-owned GC Payload objects through nested native
  default callbacks under threshold one. Every logical budget cut cleans roots,
  frames and heap objects; retained old/new script generations return 42/43 after
  reload. Missing templates and malformed parameter mutability reject at registration.
- Keep runtime ABI v139, KBC v116, KMIR v14 and helper ABI v6 unchanged. The four
  existing focused products still match exact source emission. This changes HIR's
  NativeBinding shape and raw NativeModule implementation binding behavior.

Validation actually performed: 91 SDK tests across twelve focused targets pass,
followed by the five-test source-default target including the added reload proof
(92 unique tests). Workspace library and focused source-default Clippy pass with
warnings denied. Format, structure (836 Rust files, zero findings/exceptions) and
diff checks pass. All four standalone native-proof consumers, eight production
crate boundaries and the source-free ABI build graph pass; source-enabled consumers
include the new five-test target. Manual review covers actual owner imports, empty re-export whitelist, bounded
substitution/proofs, final policy import, once-only evaluation, native target owner
materialization, encoded signature validation, callback roots and generations.
Full-workspace/all-target obsolete fixture failures remain NR04-owned as documented
in the preceding checkpoint; they were neither rerun nor disabled. Final NR05/ST06
acceptance stays open.

Next: derive default members and explicit template mappings from real Rust function
signatures, complete external declaration/template closure and projected
requirements, then restore Ord/Ordering and sort_by/sort. Managed iterator state,
full library restoration, legacy retirement and combined acceptance remain in the
active goal.


### NR02 checkpoint: typed Rust owned default templates (2026-10-01)

Checkpoint 2 now derives owned default members from actual Rust free functions.
This accepts bounded owned-trait authoring, not complete external default/template
closure, projected requirements, Ord/sorting or managed returned state. No NR phase
is accepted.

- Add `#[native_default(T: Source<P, Output = U>::echo, final)]`. The Rust trait
  contains required members only. Derive the new script member's complete signature
  from its real Rust template; explicitly invert the receiver, trait argument and
  ordinary associated-output roles into the checked NativeDefault application.
  Reordered template binders work without name guessing or repeated signatures.
- Require distinct mapped template parameters and a first receiver argument.
  Reject duplicate/unknown members, ambiguous/unmapped roles, absent associated
  types and unproved template obligations before publication. Keep member-local
  generics, associated families and generalized constraint authoring queued.
- Declare all owned default members before selected dependency resolution, native
  implementation binding and ordinary entry registration. A default can call a
  later-declared default. Native Rust implementations bind required members only;
  no fake Rust default method, generated script body or compatibility entry exists.
- Retain auxiliary templates as private registered declarations. NativeModule
  carries a bounded checked private-function identity set; HIR and generated text
  preserve that visibility while portable native declarations still carry the full
  executable contract. Templates do not grow the public free-function inventory.
- Build a new exact-emission encoded product from a real generic Source<P> trait,
  its Output slot, a required native bool implementation, final echo and overridable
  alternate defaults, and an application entry selecting alternate. It exercises
  nested default callbacks over script/native/generic receivers and returned GC
  objects under threshold one. Every budget cut cleans roots, frames and objects.
  Final/private import diagnostics, overridable script dispatch and generated member
  navigation/documentation/completion pass the source proof.
- Keep runtime ABI v139, KBC v116, KMIR v14 and helper ABI v6 unchanged. Private
  visibility is registration/source metadata; existing portable native declaration
  records already represent private templates. Old products still require the
  preceding ABI rebuild; no new wire version or legacy reader is needed here.

Validation actually performed: 100 SDK tests across thirteen focused targets and
three macro tests pass. Eight new typed-default tests include independent invalid
mapping/obligation rejection, offline nested calls, every budget cut, exact source
emission, specific final/private diagnostics, overriding and generated tooling.
All four standalone native-proof consumers, eight production boundaries and the
source-free ABI graph pass; source routes include the new default target. Workspace
library and focused test/example Clippy pass with warnings denied. Format,
structure (841 Rust files, zero findings/exceptions) and diff checks pass. Manual
review covers actual imports/empty re-export whitelist, generated runtime-path
hygiene, bounded private identities, template ownership, ordinary proof reuse,
independent binder roles, evaluation order, root cleanup and retained contracts.
The preceding NR04-owned obsolete full-workspace/all-target failures remain open;
unchanged failures were neither rerun nor disabled, and no complete-library
acceptance is claimed.

Concrete next-step reproduction (not accepted external behavior): a separate Rust
consumer declares `game::parent::Parent::read(&self) -> i32`, exports its catalog,
and authors `game::child::Child: game::parent::Parent` with an owned default
`#[native_default(T: Child::extra)] fn extra<T: NativeValue>(value: T) -> i32`.
Without a selected parameter, child API construction and composition without the
actual parent both succeed: declaration-owned parent dependencies are not retained.
Adding `#[selected(T: game::parent::Parent::read)] read: NativeSelected<(T,), i32>`
to that same template rejects API construction with "native default differs from
its registered template", even when the complete parent catalog is provided.
The builder's selected resolver retains the expected parent, but NativeApi::new
checks owned defaults before require_traits publishes that expected closure.

The observed probe command is
`cargo run --offline --manifest-path target/native-default-parent-probe/Cargo.toml --target-dir target`;
its two printed outcomes are in target/native-default-parent-probe.log. Recreate
it from the definitions above after ignored cache cleanup. The next NR02 step must
retain owned trait parents regardless of callback usage, validate defaults against
the complete expected declaration/template closure, and require actual providers
at composition/install. No unknown parent/template may become installation
authority. This is a supported-scope limitation to resolve before library closure,
not a completed external API or a reason to waive portable proof.

Next: close required external default/template contracts and concrete obligations,
then complete projected requirements and restore Ord/Ordering and sort_by/sort.
Managed iterator state, remaining library restoration, legacy retirement and
NR05/ST06 behavior/measurement acceptance remain in the active goal.

### NR02 checkpoint: complete default declaration dependencies (2026-10-01)

Checkpoint 2 closes the preceding external-parent/default-template reproduction.
It does not accept concrete implementation obligations, projected receivers,
sorting, managed returned state or a complete library. The default package now
has an explicit carried registration failure described below; no NR phase passes.

- NativeCatalog retains immutable copy-on-write trait contracts and actual native
  declarations, including private templates and their selected requirements.
  Collect referenced parents, bounds, associated constraints/projections, method
  signatures and default applications through a cycle-safe declaration walk.
  Referenced templates participate in the same walk. Retain the expected foreign
  closure independently of callback use; unrelated functions are not added to an
  entry's executable dependency set merely because they share its package.
- Change the sole low-level constructor to
  `NativeApi::new(modules, handlers, catalog)`. Collect the complete expected
  declarations before running the shared portable default proof and checking
  foreign native implementation signatures. Remove post-construction require_traits
  and the separate selected-only dependency accumulator. Existing typed
  `native_api()` / `native_api(&catalog)` authoring remains unchanged.
- Composition and staged installation require the actual owning providers and
  compare full expected trait/template records. Authoring views cannot publish
  dependency handlers. Installation failure leaves no partial traits or entries.
  Portable linking compares transitive private template declarations as well as
  public trait contracts and the invoked entry's ordinary import signature.
- Add `#[native_trait(parents("pkg::mod::Parent", ...))]` for actual Rust
  supertraits imported through short paths or aliases. Mappings are explicit and
  positional; generic arguments and associated bindings still come from the Rust
  bounds. Reject mismatched counts, repeated options, abbreviated identities and
  arguments authored again inside a mapping. This is declaration identity mapping,
  not Rust signature inference or a library-specific resolver.
- Add a real four-package Rust fixture with Parent<P>, Child<P>, Plain<P>, private
  native defaults, and a separate provider of bool implementations. Its generated
  product exercises inherited defaults and nested selected callbacks over two
  script instantiations, native receivers and returned GC objects. A coherent
  alternative Parent provider has identical public trait contracts and function
  signatures but changes echo's checked selected target from read to shift.
  Both alternatives publish independently; mixed catalogs/providers reject.
  A product calling only Plain's constant default has no Parent native imports:
  changing the transitive Parent template still rejects runtime loading, while
  the coherent alternative provider/product pair executes correctly.
- Fix ordinary compiler producers exposed by this fixture. Parent interfaces
  materialize omitted native defaults as checked native targets before queuing
  script functions. Uninstantiated generic interface rows no longer collect every
  concrete script method into duplicate slots: slots match exact implementation
  arguments. Verification keeps its existing uniqueness/signature checks. Rebuild
  the typed-default fixture for the corrected canonical emission and add the new
  external fixture and regeneration example. Other affected encoded products must
  be refreshed once their required default packages build again.
- Runtime ABI v139, KBC v116, KMIR v14 and helper ABI v6 remain unchanged. Existing
  records already carry the retained contracts. The Rust constructor breaks API
  callers; there is no compatibility alias or obsolete artifact reader.

Validation actually performed:

- 21 SDK tests across native_default_external, native_default_source and
  native_default_typed pass. The eight external tests include absent parent/helper
  rejection, independent template alternatives, atomic installation, source-free
  execution, exact source emission, every nested-callback budget cut and the
  constant-default transitive link proof. GC threshold one, roots, frames and
  post-collection object counts retain meaningful assertions.
- All three default targets pass as standalone artifact-only, source, native and
  source+native consumers. These focused runs do not replace the complete feature
  suite. Eight production crate boundaries and the source-independent ABI graph
  pass in the full native-proof runner before the carried default-package error.
- Three macro tests, the eight-case separate Rust-authoring consumer, workspace
  library Clippy and focused test/example Clippy pass with warnings denied.
  Format, whole-repository structure (846 Rust files, zero violations/exceptions)
  and diff checks pass. Manual review covers module/import ownership, the empty
  re-export whitelist, macro identity hygiene, bounded type walks, exact template
  provenance, staged publication and ordinary proof reuse.

New carried default-package error (immediate follow-up, NR03 declaration ownership):
`std::array::List<T>` has a declared `std::ops::Index<usize, Output = T>` parent,
but no native package currently owns Index's declaration. The old source-only ops
module cannot supply executable registration authority. The new complete dependency
check therefore rejects array API construction; the bundled package's existing
expect currently panics. Default Engine construction and tests selecting the
default library are broken at this intermediate checkpoint. The optional external
and typed-default proofs above explicitly disable that package, as they already
did before this checkpoint; existing default-enabled assertions remain intact.

Reproductions and observed failures:

```text
cargo test -p kagari-embed --test native_registration
# 2 pass / 11 fail: "native trait dependency is absent from the catalog"
uv run python scripts/check_features.py --native-proof
# production graphs pass; artifact-only native_associated has 15 pass / 5 fail
# at the same bundled array initialization error
```

The commands' full outputs are under target/native-default-closure-focused.log and
target/architecture-features/native-proof/artifact-only-tests.log. Existing NR04
obsolete full-workspace/all-target consumers remain additional open obligations.
No default-enabled test is disabled or made source-free to conceal the failure.
The complete runner retains every prior target and adds native_default_external.

Next, restore actual registration-owned ops declarations required by List before
further algorithm restoration. Preserve the complete ops public declaration
surface, including Range/Bound/RangeBounds/Fn; replacing its namespace with an
Index-only package would silently delete the remaining declarations. Extend the
existing native representation authoring/import/render path where needed, derive
Index from its actual Rust contract, and install its owning provider before array.
Do not bypass parent dependency checks, copy source-derived contract blobs, invent
an implicit primitive catalog, or change List's declared parent identity. Restore
default package initialization, re-emit affected current proof fixtures and rerun
the full native-proof feature matrix. Then retain actual implementation facts for
concrete default obligations and resume projected receivers, Ord/Ordering,
sort_by/sort and managed iterator state. The complete library, legacy retirement
and NR05/ST06 acceptance remain in the active unbounded goal.

### NR03 prerequisite checkpoint: owning ops declarations (2026-10-01)

This checkpoint closes the preceding default-package Index registration failure.
Default Engine construction and the complete current native-proof feature suite
work again. It accepts the restored ops declaration surface, not all ops
implementations, primitive facts or the complete library. NR02 concrete/default
obligations and projected authoring, the remaining NR03 families, NR04 migration
and NR05/ST06 final acceptance remain open.

- Add an ordinary optional std::ops NativeApi containing the complete preceding
  public surface: fifteen actual Rust required traits, six range constructors
  and generic Bound. Construct its provider before array and supply its catalog
  to array authoring. List's explicit Index parent now has an actual owning
  declaration. Remove the NativeIndex adapter trait and its macro name shortcut;
  NativeArray implements the actual ops_api::ops::Index Rust contract. Existing
  engine-owned primitive implementations are not new registered proof facts.
- Extend native_type through a checked NativeRepresentation trait implemented
  by actual resolved Rust adapters. Support aliases and single-field tuple
  wrappers; derive constructor, generic arity and enum variants without reading
  generated text or matching the spelling NativeArray. Alias bounds are checked
  by authoring/adapters and removed from emitted Rust aliases, where Rust does
  not enforce them. Unsupported scalar aliases fail Rust conformance checks;
  malformed variant payloads/counts and constructor arity reject registration.
- Add immutable NativeRange<T, Shape> proxies for all six shapes. Keep the exact
  engine integer representation, pinned call owner and checked argument/result
  types. Bridge Rust Bound<T> through actual NativeValue payload conversions,
  roots and checked enum tags; arbitrary rooted script objects remain supported.
  No unrestricted Rust reference or host object is put into the script heap.
- Import registered opaque/enum records directly into ordinary HIR. Generate
  matching enum/variant coordinates and use each module's actual local alias
  spelling for its closed representations. Mutable array import now selects its
  actual constructor instead of assuming the first type in a package is Array.
  Remove ops from the legacy source manifest and regenerate stdlib/ops.kgr as a
  tooling view. The legacy source crate remains for other unmigrated modules.
- Add an application-owned game::shapes fixture with Span/Closed/Tail/Head/
  ClosedHead/Whole/Edge Rust aliases, typed bounds and immutable range returns.
  Its product uses nested generic range calls, all bound variants, GC payloads
  and the complete u64 endpoint domain with default packages disabled. Add its
  regeneration example and native_ops to every standalone native-proof route.
  Refresh the four existing affected exact-emission products after restoring
  their default providers; do not replace assertions with relaxed comparisons.
- Fix two ordinary producer/executor bugs exposed by the new proof. Generic
  argument context could leave a range's item Unknown even after concrete
  endpoint inference, reaching lower_type's non-concrete ABI assertion. Recover
  the item from checked operands, as array inference already does. Standard enum
  Test/Read incorrectly used Bound discriminants as payload counts and type
  argument indices. Preserve arity/variant/representation checks while using
  correct Bound payload counts and the shared variant payload slot.
- Runtime ABI v139, KBC v116, KMIR v14 and helper ABI v6 remain unchanged; the wire
  already carries these constructors/shapes. Removing NativeIndex and replacing
  the hidden array_type authoring primitive are breaking Rust API changes, with
  owner imports and consumers migrated directly and no compatibility re-exports.

Validation actually performed:

- `uv run python scripts/check_features.py --native-proof` passes all thirteen
  targets in every standalone consumer: 62 artifact-only, 104 source, 62 native
  and 105 source+native tests. All eight production dependency boundaries and the
  source-independent ABI build graph pass. This restores the complete runner
  that failed at the preceding bundled Index dependency checkpoint.
- The seven new native_ops tests exercise optional installation, complete ops
  declaration coverage, source-independent execution with and without defaults,
  exact source emission, full-width endpoints, generic heap payloads, every
  budget cut, roots/frames/object cleanup, malformed representation rejection,
  static element type errors, and generated enum/variant navigation and docs.
  Existing thirteen registration and thirteen selected tests retain their
  signature, parent, output, callback and generation assertions.
- Three native macro tests and the nine-case separate Rust-authoring consumer
  pass, including renamed runtimes, aliased Bound/range types and rejection of an
  i32 native_type alias that has no NativeRepresentation implementation.
  Workspace library and focused test/example Clippy pass with warnings denied.
  Format, structure (852 Rust files, zero violations/exceptions) and diff checks
  pass. Manual review covers macro token trees, constructor/variant ownership,
  explicit imports, the empty Rust re-export whitelist, roots and exact ABI checks.
- `cargo test -p kagari-native-macros -p kagari-stdlib` confirms the three macro
  passes and the already recorded legacy package result: four pass/three fail.
  Two old fixtures expect removed intrinsic markers; map documentation is absent.
  NR04 retains these unchanged failures, obsolete all-target/workspace consumers
  and the remaining source declaration route. No failing test is disabled.

An exploratory application confirms another NR02 authoring limit. An actual Check
trait with a Rust bool implementation, Run with a usize implementation, and a
derived `#[native_default(T: Run::answer)]` template requesting
`#[selected(bool: Check::read)]` reject construction with
`typed selected bounds require a generic receiver parameter`, before default
proof. The real continuation requests `(true,)` and returns the checked i32
result; it is not a metadata-only declaration or a fake handler. The observed
command is `cargo run --offline --manifest-path
target/native-concrete-default-probe/Cargo.toml --target-dir target`, with output
in target/native-concrete-default-probe.log. Recreate that small application from
these trait/default/continuation definitions after cache cleanup. Concrete selected
receivers need ordinary owned obligations, and NativeCatalog::validate_defaults
still constructs its shared ProofCatalog without actual implementation facts.
These are next capabilities to close, not grounds to waive default validation or
claim complete Rust function/trait inference.

Next: retain and verify actual registered implementation facts and concrete
default obligations, extend selected receiver authoring through the shared checked
model, and restore Ord/Ordering plus prepared sort_by/sort. Then complete managed
returned state and the remaining library families, retire the legacy source crate
and consumers, and finish NR05/ST06 behavior matrices and matched measurements.
The original complete-library goal remains active.

### NR02 checkpoint: actual concrete implementation facts (2026-10-02)

The former concrete selected probe now constructs and executes a validated NativeApi.
This checkpoint completes the concrete-default obligation item in restoration step
2. It does not accept all NR02 capabilities or the full library. Projected receiver
and method-generic authoring, Ord/sorting, managed returned iterator state, remaining
families, NR04 migration and NR05/ST06 acceptance remain open in the active goal.

- Add an explicit Implementation source to the shared ProofCatalog: an existing
  verified executable InterfaceTableAbi or an actual registered NativeImplementation
  with its owning DefinitionId. Both use the same bounded header matching,
  conditional-obligation, associated-output, uniqueness and callable selection
  machinery. Registration creates no invented interface table, script body or
  second trait resolver. Native inherited methods apply their real trait-owned
  default template; explicit methods retain their actual registered ABI.
- NativeCatalog retains immutable actual implementation headers/methods in addition
  to traits and templates. Dependency collection follows applicable obligation
  candidates, their inherited bounds and their exact native method declarations.
  Composition and staged installation compare expected facts with actual owning
  providers. Portable linking matches retained headers and explicit methods against
  verified executable tables, whose independently checked omitted default slots may
  add materialized methods. Unrelated functions in a provider do not become callback
  requirements.
- Accept concrete typed selected receivers through the existing signature checks.
  Registered fixed predicates enter HIR with their already validated ABI binders;
  source-written bound target rules remain unchanged. Default proof still requires
  the actual concrete implementation, not just the corresponding trait declaration.
- Derive NativeModule dependency edges from the checked registration closure.
  A default may require a bool implementation from another module without importing
  any of that provider's public functions in application source. Direct HIR import
  carries an ordinary private module dependency with a fresh nonconflicting alias.
  Validate edge count/identity limits; callers cannot supply installation authority
  through generated tooling text or a declaration catalog.
- Add game::fact_contract, game::fact_provider and game::concrete_defaults as real
  Rust packages. A usize Run implementation and script Item both inherit a generic
  default whose continuation selects Check::read on bool. Exercise native/script
  direct and dynamic calls, a concrete selected default and its nested callback,
  independent source-free loading, frequent GC and every budget cut. Missing and
  changed implementation providers reject construction/composition/installation;
  failed installations publish no entries. A function named __native_dependency_0
  proves generated dependency-alias hygiene.
- Fix a common producer bug found by boxing 1usize into Run. Record the checked
  concrete receiver's semantic register type before replacing it with the boxed
  interface register. The old product labeled the input only with its physical u64
  representation, causing the ordinary access verifier to reject it. Keep that
  verifier strict; the new fixture proves forged u64 input metadata still rejects.
  Re-emit the five affected exact-comparison fixtures (associated, selected,
  provider, typed defaults and external defaults). No equality assertion is relaxed.
- Runtime ABI v139, KBC v116, KMIR v14 and helper ABI v6 stay unchanged: all wire
  records already express these facts. ProofCatalog's Rust input type now explicitly
  names its implementation source, and NativeModule includes derived dependencies.
  Migrate actual owners directly without compatibility aliases or re-exports.

Validation actually performed:

- The default-feature concrete SDK proof passes all nine tests, including exact
  emission, ordinary source-bound rejection and forged boxing-input rejection.
  Eleven ABI integration default tests pass, including inherited native/table
  agreement and conditional generic implementation matching with a negative item.
- All nine Rust-authoring cases pass. Workspace library Clippy and focused
  ABI/SDK/test/example Clippy pass with warnings denied. Formatting, whole-repository
  structure (857 Rust files, zero violations/exceptions) and diff checks pass.
  Manual review covers imports and the empty re-export whitelist, genuine fact
  ownership, bounded proof work, exact carried declarations, dependency hygiene,
  typed callbacks, staged publication and cleanup.
- `cargo test -p kagari-abi --lib --no-run` still fails in the inherited NR04 test
  consumers: removed EngineNativeBinding and old RuntimePrimitive Integer,
  ArrayListNew and string/map variants, plus dependent ValueType diagnostics
  (fourteen compiler errors). There are no remaining new ProofCatalog/matching
  migration diagnostics in that attempt. Reproduction output is under
  target/native-facts-abi-lib-build.log. The obsolete all-target/workspace consumers
  and legacy kagari-stdlib failures remain open; no test is removed or disabled.

- `uv run python scripts/check_features.py --native-proof` passes all fourteen
  targets in every standalone consumer: 69 artifact-only, 113 source, 69 native
  and 114 source+native tests. All eight production dependency boundaries and the
  source-independent ABI build graph pass. Five predecessor source-emission
  assertions initially failed solely because of the repaired semantic registers;
  after independent fixture regeneration, their original exact assertions pass
  across the complete matrix. Logs are under target/native-facts-features.log and
  target/architecture-features/native-proof/. Other focused outputs are under
  target/native-concrete-tests.log, target/native-facts-abi-tests.log,
  target/native-facts-authoring.log, target/native-facts-clippy.log and
  target/native-facts-focused-clippy.log.

The original unbounded full-library goal remains active. Next, extend projected
receiver authoring through actual registered contracts and restore the complete
cmp declaration/provider surface and prepared sorting. Follow with managed
returned state, remaining families, legacy retirement and NR05/ST06 acceptance.

### NR02 checkpoint: ordinary projected selected receivers (2026-10-02)

Restoration step 2 now accepts an ordinary qualified associated receiver in a
typed selection marker. This closes the projected receiver substep; Ord/Ordering,
prepared sorting, managed returned state, remaining families and final acceptance
remain open in the original unbounded goal. Associated families and method-generic
selected authoring are not claimed by this proof.

- Rust authors keep actual `T: NativeValue, U: NativeValue` value parameters and a
  `NativeSelected<(U,), i32>` handle. The marker
  `#[selected(<T as game::selected::Echo<Output = U>>::Output:
  game::selected::Echo<Output = i32>::echo)]` names the script projection explicitly;
  it does not require a fictitious Rust Echo implementation for GenericValue.
  Only selection-marker receivers use this syntax. Ordinary Rust function/impl
  types retain their actual Rust NativeValue conversion and trait conformance.
- Resolve the real owning trait and ordinary associated declaration from the local
  module or explicit NativeCatalog. Reject absent members, undeclared output
  bindings, unsupported families and incompatible typed callback signatures before
  publication. Derive each projected receiver's base obligation as well as its
  selected output obligation. An embedded output equality alone cannot supply the
  base receiver's trait authority.
- During generic forwarding inference, use a unique compatible caller trait bound
  to constrain associated equalities before searching concrete implementations.
  The checked pass still validates applicability; missing caller bounds and
  incompatible equalities remain ordinary analysis errors.
- Preserve registration-owned NativeDeclaration templates exactly in executable
  ABI. HIR normalizes bounds for static checking; mixing those normalized bounds
  with original selected requirements produced `InvalidPublicAbi`. Preserving only
  the declaration then exposed `InvalidGraph`, because direct-call requirements
  still came from normalized HIR. Emit both template and applied requirements from
  the same registered facts while retaining checked call signatures. Legacy
  nonregistered entries keep their existing HIR-owned route pending NR04. Portable
  validators and exact runtime contract matching remain unchanged and strict.
- The real game::projected package consumes game::selected's actual Echo/Bag
  contracts. `echo_twice` resumes one callback, roots its associated output, forces
  allocation and invokes the selected member on that output. `inspect_output`
  exercises the implied base constraint without requesting a base callback.
  Native/native, script/script, mixed and generic forwarding entries return 42.
  Compare generated tooling text, implied bounds and the carried declarations with
  actual registration, without parsing generated text to establish semantics.
- Encoded/source-free tests reject erased projected obligations and a changed
  second selected target. Runtime tests cover frequent GC, every budget cut,
  traps in either callback and retained script generations across reload.
  Registration negatives cover an absent catalog, absent associated member and
  wrong callback result. Source negatives cover missing bounds, contradictory output
  equalities and a projection whose base receiver has no applicable implementation.
- Runtime ABI v139, KBC v116, KMIR v14 and helper ABI v6 stay unchanged. The existing
  wire types already express these projections, predicates and concrete targets.
  A new independently regenerable native_projected fixture joins the standalone
  proof matrix; existing exact fixture assertions are retained.

Validation actually performed:

- `uv run python scripts/check_features.py --native-proof` passes all fifteen
  targets in every standalone consumer: 74 artifact-only, 122 source, 74 native
  and 123 source+native tests. The projected proof contributes five offline tests
  and four source tests. All eight production dependency boundaries and the
  source-independent ABI build graph pass. Existing source-emission byte equality
  assertions pass without regenerating older products.
- Four native-macro unit tests pass, including rejection of unqualified,
  multi-member and family projection syntax. Workspace library Clippy and focused
  projected SDK/test/example Clippy pass with warnings denied. Formatting, the
  whole-repository structure check (860 Rust files, zero violations/exceptions)
  and diff checks pass. The added tooling-coordinate assertion initially assumed
  the reverse canonical order of the two bounds; correct the assertion to the
  actual sorted contract and rerun the entire feature matrix successfully.
- Manual review covers explicit owner imports and the empty re-export whitelist,
  exact registered template/application retention, genuine declaration ownership,
  generic inference and negative applicability checks, callback root lifetimes,
  cleanup and generation retention. No forwarding API, synthesized script body,
  alternate trait solver or validation bypass is added.
- Full all-target/workspace acceptance is still blocked by the inherited NR04
  legacy test/example consumers and remaining library restoration. The preceding
  checkpoint's fourteen ABI lib-test compiler errors still belong to NR04; this
  checkpoint does not disable those tests or repeat an unchanged failing build.
  See target/native-facts-abi-lib-build.log and the preceding ledger entry for the
  reproduction and diagnostics.

New outputs are under target/native-projected-features.log,
target/architecture-features/native-proof/, target/native-projected-tests.log,
target/native-projected-macros.log, target/native-projected-clippy.log,
target/native-projected-focused-clippy.log and target/native-projected-structure.log.

The original goal remains active. Next, restore the complete cmp declarations,
actual selected ordering implementations and prepared sort_by/sort. Follow with
managed returned state, remaining families, legacy retirement and NR05/ST06
acceptance. This checkpoint accepts no entire NR phase.
