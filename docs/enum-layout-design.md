# Enum layouts: inline values and niche encoding

Status: deferred at the user's request on 2026-11-08, pending Runtime execution
and ownership review. EL00 model/baseline work is locally complete and retained;
production enum storage is unchanged. EL01-EL04 remain unstarted and inactive.
Baseline: `7837fed4`. The [roadmap](implementation-roadmap.md#enum-layouts)
owns activation. This document owns the proposed architecture, finite delivery
sequence and acceptance gates until completion or replacement.

The API sketches and EL00 layout choices are research inputs for later review,
not requirements imposed on the upcoming Runtime refactor. Before reactivation,
recheck the transport/layout decisions and refresh affected cost baselines against
the resulting execution model. Value structs remain separately deferred.

Runtime detachment has withdrawn nominal collection backends. The EL00/enum
measurement Rust consumer now uses checked script observers and compiles, but its
Vec-based workload awaits NC reconnection. Historical `(retired)` paths and old
measurements are not a current provider or performance claim.

## Purpose and scope

Introduce a common enum layout mechanism covering Option, Result, small standard
enums and eligible user enums. Keep their existing nominal types, constructors,
matching, trait implementations and propagation syntax. Representation selection
depends on checked variant/field layouts, not whether a type is named Option.

Two related changes are required in this delivery:

- Inline eligible enum values to remove per-value GC wrapper allocations.
- Encode discriminants in proved unused payload states where that reduces the
  layout; propagate remaining niches so nested enum layouts can compose.

Explicit-tag inline layouts are the correctness fallback when no niche exists.
Niche encoding is required scope, not an optional optimization after this work.
Large or recursively expanding layouts retain a checked heap representation.
The inline eligibility budget and transport decisions are recorded below; the small
Option/Result and niche acceptance cases below are mandatory, not samples that
may all fall back to the heap and still count as delivery.

An eligible enum must not allocate a wrapper when constructed, copied, matched,
passed through shared generic code, stored or converted after layout preparation.
This excludes allocations intrinsic to its payload, Err origin capture, explicit
interface boxing, host DTO conversion and backing-storage capacity growth. Those
costs must be measured separately, not relabeled as wrapper elimination.

Explicit [value structs](implementation-roadmap.md#deferred-value-structs) remain
deferred and are not a prerequisite. Ordinary structs retain shared identity.
General tuple/struct inlining, a new collector, implicit conversions, new script
syntax and importing Rust ownership/ABI rules are outside EL00-EL04.

## Current behavior and diagnosis

The existing [value contract](spec/value-semantics.md#values-and-identity) gives
enums value semantics and immutable payload membership. Referenced mutable
objects keep shared identity. Removing an enum wrapper need not change semantics.

Current physical ownership is nevertheless uniformly heap-based:

- [Semantic representation](../crates/kagari-contract/src/representation.rs)
  maps enums to `ValueType::HeapObject`.
- [Enum layouts](../crates/kagari-contract/src/layout.rs) retain nominal type
  arguments, variant identities, payload types and `reports_failure` facts.
- [Carriers](../crates/kagari-runtime/src/value/carriers.rs) transport scoped,
  stored or Host-rooted managed values; the actual
  [EnumObject provider](../crates/kagari-runtime/src/value/aggregates/enumerations.rs)
  contains tag, traced payload and optional origin.
- [Enum construction](../crates/kagari-runtime/src/native/conversion/context/enumerations.rs) validates payloads,
  captures an origin for marked variants, then allocates the enum object.
  Both Some/None and Ok/Err use this general route. Standard constructors and
  native conversion additionally assume heap enum views.
- [Execution operands](../crates/kagari-runtime/src/frame/values/operands.rs)
  use scalar banks and traced StoredValue banks. Scalar initialization bits mean
  written/unwritten; they cannot be repurposed as enum presence bits.
- [Struct fields](../crates/kagari-runtime/src/module/layout_storage.rs) and
  `crates/kagari-runtime/src/native/sequence.rs` (retired) specialize primitive
  storage but otherwise rely on managed representations.

For an iterator producing Some(42), or a function returning Ok(42), the caller
currently follows a handle to an enum object just to inspect a variant and integer.
An inline layout can carry that state and integer directly. For Some(player),
the original player's handle remains; only the outer enum allocation disappears.

This is a representation-model mismatch, not primarily an allocator defect.
Caching None, optimizing only stdlib constructors or duplicating Option/Result
semantics in VM handlers would leave source, native and generic routes inconsistent.
These are source-derived observations; this design has no measured speedup.

## Reference mechanisms and differing assumptions

[Rust 1.90.0 enum layout calculation](https://github.com/rust-lang/rust/blob/1.90.0/compiler/rustc_abi/src/layout.rs)
computes variant layouts and compares direct-tag and niche-filling candidates.
The niche candidate reserves invalid scalar states for other variants, checks
their payloads fit, and exposes remaining niches. Adopt checked, compositional
layout selection and explicit-tag fallback. Kagari need not reproduce rustc's
search heuristics or promise the same layout choices.

[Rust 1.90.0 Option representation](https://github.com/rust-lang/rust/blob/1.90.0/library/core/src/option.rs)
documents guarantees for selected non-null/nonzero payloads. This does not make
all Rust enum layouts stable, supply niches to full-width integers, or authorize
transmuting Kagari values into Rust Option/Result bytes. Kagari also needs precise
roots, shared generic transport, executable verification and reload provenance.

The [.NET 9.0.0 Nullable implementation](https://github.com/dotnet/runtime/blob/v9.0.0/src/libraries/System.Private.CoreLib/src/System/Nullable.cs)
illustrates explicit presence plus inline payload when no niche is available.
Its value-type constraints and special boxing behavior do not define Kagari's
nominal enum, trait or interface semantics.

Kagari's custom part is the bounded generic transport and retention of applied
layout/origin metadata. Rust's typed lowering provides layout guidance, not an
implementation of those managed-runtime ownership obligations. The following
primary-source review supplements EL00 before the production cutover; it changes
no implementation or deferred value-struct scope.

### Shared generics, calls and containers in other runtimes

**Rust.** The [compiler monomorphization guide](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html)
(reviewed 2026-11-08) describes generating concrete generic instances. Combined
with the pinned 1.90.0 layout implementation above, this lets concrete element
layouts drive code generation. Its approach does not establish an allocation-free
ABI for Kagari's shared bodies or a stable native ABI. Preserve checked layout
facts without requiring Rust-style monomorphization throughout Kagari.

**Swift 6.0.3.** Its [enum ABI layout](https://github.com/swiftlang/swift/blob/swift-6.0.3-RELEASE/docs/ABI/TypeLayout.rst)
composes unused payload encodings, exposes remaining extra inhabitants, and adds
tags where necessary. The same document distinguishes size, alignment and array
stride; its three-pointer existential buffer is an erasure-container mechanism,
not a universal limit on ordinary values. This is a useful comparison for both
the enum layout algorithm and why Kagari's 24-byte eligibility policy remains a
local bounded-delivery choice rather than an ABI requirement.

Swift's [type metadata](https://github.com/swiftlang/swift/blob/swift-6.0.3-RELEASE/docs/ABI/TypeMetadata.rst)
points to shared value-witness operations and layout properties. The
[witness declarations](https://github.com/swiftlang/swift/blob/swift-6.0.3-RELEASE/include/swift/ABI/ValueWitness.def)
include copy/assignment/destruction, size/stride/alignment information and enum
tag access. Adapt the separation of type-owned layout/operations from value bytes;
Kagari needs tracing/admission and origin retention instead of copying Swift's
ARC ownership operations or raw metadata-pointer identity.

The pinned [generic tuple IRGen test](https://github.com/swiftlang/swift/blob/swift-6.0.3-RELEASE/test/IRGen/generic_tuples.swift)
checks an unspecialized generic function receiving input/output addresses and
type metadata. It obtains a size from the witness table, allocates a local stack
temporary and copies into caller-provided results through witnesses. The
[generic witness-method test](https://github.com/swiftlang/swift/blob/swift-6.0.3-RELEASE/test/IRGen/witness_method.sil)
also checks indirect results. These implementation tests corroborate the
mechanism without treating the older calling-convention whitepaper as a complete
current ABI specification. Indirect argument/result transport does not by itself
mean a separately allocated heap wrapper.

Swift's [contiguous array buffer](https://github.com/swiftlang/swift/blob/swift-6.0.3-RELEASE/stdlib/public/core/ContiguousArrayBuffer.swift)
allocates tail elements using their Element type, projects typed element storage
and performs generic element operations. Array growth/count/capacity logic does
not implement a second Optional/Result encoding algorithm. Kagari's corresponding
boundary should be a Runtime-owned typed storage protocol used by Vec, with
descriptor-directed construction/copy/scan rather than enum-name branches in
each container. Swift's copy-on-write array and ARC lifetime rules are different
from Kagari's shared mutable collections and tracing GC.

**CoreCLR/.NET 9.0.0.** The [CLR ABI](https://github.com/dotnet/runtime/blob/v9.0.0/docs/design/coreclr/botr/clr-abi.md)
passes value types directly or indirectly according to target rules. For example,
Windows AMD64 ordinary JIT calls can pass by-value structs through stack
temporaries; indirect result buffers and managed-pointer reporting have their
own contracts. Adapt the distinction between value semantics, physical ABI and
GC visibility, not its exact register thresholds or interior-pointer rules.
This is not evidence that CLR can overlay references and integers arbitrarily
or reuse Kagari's tag-dependent scan recipes.

Its [shared generics design](https://github.com/dotnet/runtime/blob/v9.0.0/docs/design/coreclr/botr/shared-generics.md)
describes reference-type code sharing with dictionaries and separate code bodies
for value-type instantiations. That is a different tradeoff from Kagari's shared
value-generic execution; NativeAOT may differ and is not generalized from this
CoreCLR document. The [List<T> implementation](https://github.com/dotnet/runtime/blob/v9.0.0/src/libraries/System.Private.CoreLib/src/System/Collections/Generic/List.cs)
stores T[] and owns capacity/version logic, leaving element representation to
the runtime/type system. C# nullable references and Nullable<T> value types are
different mechanisms; Nullable<T> is not a general Option over class references.

**Kotlin/JVM and JDK 25.** The official [inline-class representation guide](https://kotlinlang.org/docs/inline-classes.html)
(reviewed 2026-11-08) demonstrates unboxed concrete value-class parameters but
boxing at generic/interface boundaries in its examples. A concrete no-wrapper
case therefore does not prove generic/container behavior. In
[OpenJDK 25 Optional](https://github.com/openjdk/jdk/blob/jdk-25-ga/src/java.base/share/classes/java/util/Optional.java),
the implementation is an object with a reference field; empty() reuses an empty
instance while of(value) constructs a wrapper. The
[JDK 25 escape-analysis documentation](https://docs.oracle.com/en/java/javase/25/vm/java-hotspot-virtual-machine-performance-enhancements.html)
describes conditional scalar replacement. Do not mistake optimizer-dependent
elimination for the predictable interpreter/source-free representation required
here, or generalize this pinned JDK baseline into a claim about later Valhalla work.

**Implications for EL01.** Treat semantic value layout, owner storage/array stride
and physical call ABI as separate checked facts. A compact nine-byte enum record
does not require passing exactly nine bytes in native registers, and passing its
address does not require a GC allocation. Known concrete calls may use direct
lowering; shared generic calls can use admitted locations, type metadata and
prepared result storage. Rooted data must outlive the call and every safepoint;
temporary byte views still expire before reentry/collection.

The current packed alignment-one format is a proposed internal byte-record
policy, not a foreign ABI or a rule adopted from Swift/CLR. EL01 must explicitly
represent size/alignment/stride and native marshalling, and EL03 must compare the
packed policy with an appropriately aligned candidate before claiming a cost
win. Apply the existing gates to those alternatives. Do not infer from this
source review alone that the measured 24-byte cutoff is optimal, that larger
ordinary values inherently require boxing, or that Kagari's hot-reload/origin
lifetime safety has been proved by another runtime's implementation.

## Semantic invariants

1. Every enum keeps its declaration, applied arguments, variant identities and
   ordinary protocol behavior. Equal physical layouts never imply equal types.
2. Payload expressions evaluate left to right once. Matching and `?` preserve
   selected Try/FromResidual/From calls, traps, completed effects and cancellation.
   Removing allocations must not remove required execution polling checkpoints.
3. Nested variants remain distinct: None, Some(None) and Some(Some(x)) cannot
   collapse. Nested Result and mixed Option/Result combinations keep all branches.
4. Payload membership is immutable. Copying object-bearing enums preserves the
   referenced object's identity; it does not clone or freeze the object.
5. Error origins follow [error reporting](spec/error-reporting.md), including
   fresh capture, forwarding and nested failure behavior, regardless of layout.
6. Scoped host views and other non-storable payloads remain non-storable through
   every wrapper. Inline representation cannot turn a borrowed view into ownership.
7. Equality, hashing and pattern selection consume semantic variants and selected
   implementations. Raw storage bytes, inactive payloads and descriptor token
   numbers are not equality or hash oracles.

## Layout ownership and preparation

Keep HIR types nominal. `kagari-contract` owns the declarative representation
policy, validity rules and independent verification of enum layout evidence.
The compiler consumes checked instantiated fields and semantic facts; backends
consume verified layout operations without re-resolving syntax/types.

Runtime owns prepared applied descriptors and target storage adapters. Shared
generic bodies obtain the closed descriptor from their admitted type environment;
they do not inspect the first value or perform per-value trait/name lookup.
Equivalent user enum shapes receive the same eligibility analysis as core enums.
Core identity still matters for language/library contracts, but not for deciding
which enum shape may use inline or niche storage.

A descriptor retains nominal installation/type provenance, variant/field mappings,
storage size/alignment, initialization/validity rules, encoding, GC/metadata scan
recipes, error-origin locations and the remaining usable niches. A runtime token
must detect foreign owners and stale/reused descriptors even for payload-free
variants. Preparation reuses existing applied-layout/type-environment owners;
do not add a permanent global cache or retain executable bodies just for types.

Portable facts contain checked type/layout recipes, not target pointers or runtime
tokens. Closed layouts are prepared once per admitted application; open generic
recipes remain symbolic until checked substitution. Budget arithmetic, field
offsets, alignment and recursion are validated under existing bounded admission.

## Representation selection

The following names describe candidate encodings, not proposed public Rust APIs:

```text
checked nominal enum + substituted field layouts + failure-origin requirements
                              |
                       layout selection
                              |
          DirectInline / NicheInline / Heap
```

1. Build variant records from payload fields and any required origin slot.
   Unit variants need no payload bytes; uninhabited branches cannot be constructed.
2. Build an explicit-tag candidate with overlapping storage for alternative
   variants, preserving each variant's own field offsets and alignment.
3. Seek a niche candidate using proved invalid encoded states and payload-fit
   checks. Never overwrite a live field or an active origin record with a tag.
4. Choose deterministically using size, alignment and supported transport/scan
   costs. If neither fits the frozen inline budget, retain the heap layout.

Nested inline enums expose only their remaining niches. For recursive type
cycles, descriptor construction detects the cycle and inserts a heap boundary
before attempting infinite expansion. Budget/heap choices depend on the applied
type, not current value, chosen variant or first collection element. Different
variants of one applied type use one coherent representation contract.

Initial delivery includes payload-free enums and bounded single/multiple-field
variant records over existing scalar/managed fields and admitted inline enums.
Ordinary structs/tuples remain handles when used as fields. This supports Option,
Result, Ordering, ControlFlow and equivalent user shapes without promising that
every application or recursively expanding payload is inline.

### Niche rules and examples

A niche is outside the validity domain of the *encoded storage*, not an invalid
Rust value that may be temporarily constructed. Decode the enclosing enum before
constructing a bool, char, managed handle or child enum. Inactive storage is
initialized canonically and is never decoded or traced as a live field.

| Type/shape | Candidate behavior; illustrative, not a target-size guarantee |
| --- | --- |
| Payload-free Ordering or a user color enum | Inline variant number; unused encodings remain available |
| Option<bool> or a same-shape user enum | Two bool states plus one absence encoding |
| Option<Ordering> | Reuse an unused child discriminant encoding |
| Option<Option<bool>> | Four distinct states; consume another remaining child niche |
| Option<u64> | Explicit tag plus all integer bits; zero remains a valid Some payload |
| Result<u64,u64> | Explicit tag plus overlapping integer storage, with Err origin handled separately |
| Optional managed handle | Reuse a reserved encoding only after proving it cannot be a valid live handle |

Required niche families in this delivery are encoded bool, Unicode-scalar invalid
ranges and unused enum discriminants, including compositional nested enums.
Managed-handle niches require a concrete reserved-state proof. EL00 selected
direct tags for those fields without losing wrapper elimination. Do not steal
owner/generation bits or assume null/alignment rules
from Rust pointers. Full-width integers/floats retain all valid bits; no zero,
NaN, negative-zero or user-constructor invariant supplies a niche by convention.

Niche capacity and reservation arithmetic are checked. Exhausting a child's
niches falls back to direct encoding. `Option<Infallible>` and similar residuals
have only their actually inhabited states; the encoding cannot manufacture a
payload of an uninhabited type. Byte-size gains must be measured in specialized
field/element storage, separately from fixed-size generic transport.

## Result error-origin ownership

The existing checked `reports_failure` fact, not a short name such as Result or
Err, selects capture behavior. General layout supports marked variants as well
as ordinary enums. Representation selection must account for this fact and
cannot silently route all marked variants to the heap to avoid implementing it.

An inline marked variant carries a checked reference/token to an immutable origin
record retained by Runtime's diagnostic metadata owner. Use the eight-byte
non-reused tokens and traced reclamation selected below. It is not a new GC enum wrapper or an
untracked raw Arc pointer. A newly captured record may allocate; retaining an
already captured record while copying/forwarding the value must not allocate a
fresh record per propagation. Runtime's traversal/lease model must retain and
reclaim records even if the transport remains Copy.

- Constructing Ok does not capture an origin for the outer Result.
- Constructing a new Err captures the current origin and stack once, after
  admission and before publishing the fully initialized result.
- Assignment, return, storage, `?` and standard forwarding preserve the original
  record. `map_err`/From conversion can change payload without changing that origin.
- Reconstructing Err after destructuring captures a new origin as specified.
- `Some(Err(e))` and `Ok(Err(e))` retain the child's origin, but do not turn it into
  the outer value's failure. `Result<Result<T,E>,F>` may need separate origins
  at different layers; one global origin slot would lose this information.
- Equality and hashing ignore origin data. Traps during conversion remain traps.

Origin slots follow variant activity and nested layout structure; generic copying,
projection and niche encoding must preserve all active records. Retaining origin
records must not retain script values, obsolete whole programs or a self-rooted
metadata table. Error-record allocations and payload allocations are measured
separately from the eliminated enum-wrapper allocation.

## Canonical transport, storage and enum operations

The logical inline value is an applied descriptor plus initialized encoded storage.
Storage can contain multiple active object edges and origin references; a single
`Value::object_id()` lookup is no longer a complete retention protocol. This is
not a recursive `Value` field and does not allocate Box/Arc payloads per wrapper.

EL00 rejects a uniformly widened Value: the measured 32-byte inline carrier
becomes 40 bytes when added beside existing Value variants, versus 16 bytes for
the current transport. Select typed packed storage with temporary views instead.
Existing scalar banks, primitive sequence specialization and 16-byte non-enum
Value transport remain separate from inline enum records. EL01 must replace
universal `Value` assumptions in affected operations, not insert per-enum side
records behind the old API. This is a required ownership migration, not a claim
that the current Value APIs already support views.

Freeze encoded inline records at at most 24 bytes, alignment one, excluding the
shared descriptor reference. Existing byte codecs already avoid native typed
references; enum record access must likewise copy scalar bits/complete managed
IDs through checked slices. Use native-endian scalar bytes internally, with
explicit native/AOT marshalling; never cast packed bytes into Rust references.
Direct discriminants use the smallest checked 1/2/4-byte width for inhabited
variants. EL00's executable model samples the one-byte case, not the full loader.

Freeze descriptor and origin tokens at eight bytes each, issued from one checked
process-wide monotonic identity space without reuse. A counter is not a global
metadata cache: records live only in their owning Runtime tables. Lookup in the
owning table plus immutable provenance admission detects foreign/stale tokens;
counter exhaustion fails before publication. Heap handles retain their existing
full ownership/generation validation. Origin tokens occupy active variant fields
within the 24-byte budget; a root/slot/container's descriptor sits outside its
element bytes. Transient owned snapshots may use descriptor plus 24-byte stack
storage; they cannot widen every persistent Value or become untraced values.
Logical slot/element bounds remain independent of byte length: a zero-byte
Option<Infallible> record still has type provenance and a checked logical index.

Slot/field/element owners retain bytes and the applied descriptor. A borrowed view
contains the descriptor and a checked byte slice (24 bytes on the measured x64
host) and expires before collection, allocation, callbacks or storage growth.
Across those boundaries, reuse admitted frame/window or rooted object locations
with generation checks, and copy into a prepared typed destination/root pack.
Never carry a raw byte pointer through reentry or allocate a wrapper to extend a
view's lifetime. Shared generic frame admission resolves physical storage once
from its type environment; a generic scalar stays in scalar/value storage.
Native argument/result adapters must use checked projections and typed destination
publication instead of universally returning an owned raw Value. Host escapes use
the existing explicit-root boundary, extended to own typed records.

Determine recursive fallback with one bounded pass over nominal by-value enum
dependencies before recursive substitution; every declaration in a cyclic
component uses the heap representation. Managed struct/tuple/container fields
break that dependency chain. This prevents both infinite type-growing expansion
and inconsistent inline-root/heap-child representations of the same recursive
application. Nonrecursive parents can inline a reference to that heap value.

Reserve bool/char/enum-discriminant niches, consuming a checked range and exposing
the remainder. The initial search covers one payload-bearing variant plus
payload-free alternatives after uninhabited branches are excluded. Other shapes
use direct tags; more elaborate rustc-style payload-fit searches are not needed
for this delivery. No managed-handle niche is selected: preserve full handle
bits and use direct tags until a separately justified reservation exists.

Preserve one enum construction/discriminant/payload-projection semantic path with
encoding-specific handlers. Replace APIs that require an enum heap ID with a
representation-independent checked view. Inspecting an inline enum must not
materialize a heap enum. Use borrowed/projected field views and bounded argument
storage rather than allocating a Rust Vec on each construction or match. Explicit
owned diagnostic snapshots can retain their documented allocation behavior.

Shared generic inputs/results use the same typed storage/views and the admitted
descriptor. Conversions to/from specialized fields and slots cannot rebox an
eligible enum. A bare untyped None, Ok integer or discriminant is insufficient
when the caller frame has disappeared: reflection and host checks still need the
complete applied enum type and supplying installation.

Struct fields and homogeneous sequence storage retain shared descriptors and
store encoded records at validated offsets. Maps, sets, tuples and other traced
storage owners must gain typed record support instead of reboxing; keep existing
primitive specialization.
Dedicated container bitmaps and unrelated collection-storage rewrites are excluded.

Update existing Rust Option/Result DTO and native variant converters, reflection,
debugger formatting and equality/hash consumers through the common view. Preserve
conversion budgets, roots, visibility, scoped borrows and callback/reentry cleanup.
An explicit interface box remains a distinct object; construction of the enum
inside it must not require an additional hidden enum object.

Interpreter and native/AOT execution agree on representation, admission, copying,
origin retention and GC maps. Actual native execution is required evidence;
silent interpreter fallback does not establish support. Target-specific byte
packing is internal; it does not establish compatibility with Rust Option/Result ABI.

### Native values and retained native payloads

The shared boundary is a typed value protocol, not a Vec-specific byte format.
Logically a native-facing value combines its checked applied type/layout, an
admitted storage location and a lifetime/retention owner. The call signature or
container already retains much of this evidence; do not duplicate a full layout
record or allocate a root for every element merely to pass it to native code.
The following names describe responsibilities, not accepted public API spellings.

- A scoped argument projection borrows already rooted caller storage. It permits
  validated inspection/copy into a typed destination and cannot escape the call
  or remain borrowed through callbacks/collection. Reacquire by admitted location
  after reentry; do not expose unchecked `(layout, pointer)` construction.
- The native result is published into a prepared, type-checked result destination.
  Constructing Some/Ok writes its record there; it does not return a new heap enum
  or require a universal owned Value large enough for every representation.
- An escaping host value retains an explicit typed root. A native collection
  instead owns bulk typed storage and traces its records as payload edges; it
  must not register an independent external root for every element, which would
  hide cycles and add per-element root costs.
- Typed Rust bindings keep their ordinary Option<T>/Result<T,E> conversions where
  T/E have registered mappings. Generic low-level bindings operate through the
  scoped/retained protocol. A script Player remains a checked script-object
  handle unless the caller explicitly requests an owned DTO conversion.

Current [NativeEntry](../crates/kagari-runtime/src/native/binding.rs) returns Value,
and [NativePayload::trace](../crates/kagari-runtime/src/native/storage.rs) visits
references to Value. Both contracts must change coherently: the former must
support checked typed result publication, and the latter must expose typed value
regions/records to Runtime scanning, including metadata/origin edges. Merely
adding byte storage to stdlib collections leaves these boundaries incomplete.
Existing ArgumentView, StorageContext, StorageType, explicit roots and native
payload ownership are foundations to adapt, not a reason to add parallel native
registration, unchecked provider memory access or collection-only capabilities.
EL01 should define these contracts before migrating concrete container storage.

### Call-frame model and independent libraries

Design review, 2026-11-08: the user reaffirmed fully independent stdlib ownership
and identified registration/calling complexity and the lack of a clear Runtime
mental model. This expands the architecture review beyond enum encoding. The
proposal here is to make one call-frame protocol the host boundary, rather than
add isolated helper APIs around current collection privileges. Implementation of
the registration redesign, NC ownership migration and collector replacement is
not activated by this review. Earlier usage sketches below remain exploratory;
their public spelling and separate context/writer ceremony are not frozen APIs.

Current Runtime/Host ownership is already implemented. NativeCall borrows
protected source arguments, one prepared binding, a pending result owner and
exclusive services; synchronous calls/reentry/GC use that same model. Runtime has
no stdlib dependency or privileged collection operations. EL01 must rebase its
older conceptual sketches below on the [actual call model](architecture/runtime/calls.md)
and [public payload protocol](spec/host-interop.md#native-payload-accounting).
This enum proposal does not reopen completed Runtime/GC/Native architecture.

The proposed public mental model has three concepts under Runtime:

| Concept | Contract |
| --- | --- |
| Module | Declares callable/type contracts and attaches local Rust implementations; installation checks the finished module |
| Call frame | Contains admitted arguments, protected temporaries, one declared result and checked services for nested calls/value access |
| Retained handle | Keeps a script value and its exact type dependencies valid beyond a frame; no unrestricted Rust reference into script storage |

Both script-to-host and host-to-script calls use the same logical sequence:

```text
select callable and admit arguments
    -> establish the callee frame and type environment
    -> execute script body or native entry
    -> validate/publish the declared result
    -> retire frame temporaries on return or failure
```

This is a logical API contract, not a requirement to allocate another frame or
copy already protected arguments on every crossing. The host cannot observe
interpreter register banks or native machine calling conventions. Argument and
temporary locations remain valid across admitted nested calls without exposing
addresses; short storage views still end before conflicting activity. Completed
effects, cleanup, cancellation and ordinary current-body selection are preserved.

Lua supplies a useful precedent:
[Lua 5.4's API stack](https://lua.org/manual/5.4/manual.html#4.1) gives native
callbacks arguments, temporary values and results in a single scoped window.
[Lua 5.4.8 API implementation](https://github.com/lua/lua/blob/v5.4.8/lapi.c)
implements call and value operations over that window; the
[collector](https://github.com/lua/lua/blob/v5.4.8/lgc.c) traverses thread stacks.
[mlua 0.11.4 create_function](https://github.com/mlua-rs/mlua/blob/v0.11.4/src/state.rs#L1187-L1196)
adapts typed Rust arguments/results to the underlying stack callback. This shows
that ergonomic Rust callbacks can share one low-level model without requiring
ordinary callers to manually manipulate stack positions.

Kagari differs: signatures are static, the script result is one declared value,
nominal generic identities must be admitted, and reload retains supplying type
environments. Use checked typed slots and descriptor-directed representations;
do not import dynamic result counts, Lua's error unwinding, a uniform boxed stack
cell or Lua's GC policy. Preserve Rust destruction on failure. The reference
supports the frame/adapter separation, not Kagari's enum sizes or performance.

Candidate ergonomic direction, not currently callable APIs:

```text
// Ordinary Rust types supply an unambiguous declaration and one native adapter.
module.function("max", |a: i64, b: i64| -> i64 { a.max(b) })?;

// Generic/nominal declarations remain explicit; use the same registration owner.
module.function(generic_identity_declaration, identity)?;

fn identity(call: &mut CallContext<'_>) -> NativeResult<()> {
    let value = call.arg(0)?;
    call.return_value(value)
}

// The existing typed SDK call remains a convenience over the same call protocol.
let function = runtime.function::<(i64, i64), i64>(&program.root(), "choose")?;
let result = runtime.call(&function, (3, 7))?;
```

The proposed function registration operation produces a checked declaration plus
a locally bound entry, not a second registry or an executable-only declaration.
Typed adapters synthesize finite portable facts before compilation/loading; raw
generic entries require authored facts. Rust Object/ScriptValue mappings alone
cannot infer a specific nominal script declaration or generic bound. Explicit
declaration attachment remains available for those cases. Portable declarations
are still serializable independently of Rust closures and source analysis.
The proposed arg/return_value operations are a reduced view of the same
argument/result protocol below, not a parallel Value transport. Exact Rust trait
inference and public signatures remain review work; do not add forwarding APIs
merely to keep both exploratory spellings.

Library independence is an execution/ownership requirement: stdlib owns concrete
Vec/HashMap/HashSet declarations, storage, algorithms, cursors and host adapters.
Runtime owns generic nominal admission, call frames, layouts, object identity,
roots, checked loans, tracing/publication and accounting. Core fixed arrays and
other language primitives remain language-owned; they must not recognize Vec as
a privileged library implementation. Declared protocol calls may be optimized
without introducing concrete VecIndex/Map/Set engine authority.

Native payload integration is orthogonal to the call model. An independently
implemented collection supplies audited tracing/storage evidence through the
same provider boundary as any application. Ordinary callers use its safe methods;
they do not manage payload loans/barriers for each operation. The earlier
with_payload_edit sketch is not a primitive that unifies calls, nor an existing
collector capability. The NC/GC plans retain ownership of the general provider
protocol and its coordinated migration; no redesign of the collector algorithm
is assumed just to establish the call-frame mental model.

Before freezing EL01's public host transport, review one end-to-end witness at
this boundary: ordinary function registration, generic Option<Player> forwarding,
a nested script callback, explicit retention across calls and a container in a
separate crate. Keep that crate on the same public path used by stdlib. Record
the finite removal/consumer migration at the NC owner rather than adding a
parallel runtime rewrite plan. A passing enum cutover against current special
paths cannot count as completion of independent-stdlib acceptance.

### Native API precedents

These references establish separate mechanisms, not a drop-in implementation of
Kagari's complete native boundary. Versions are pinned for review.

| Reference | Concrete interface | Relevant mechanism and limit |
| --- | --- | --- |
| [Wasmtime 32.0.0 host functions](https://github.com/bytecodealliance/wasmtime/blob/v32.0.0/crates/wasmtime/src/runtime/func.rs#L401-L405) | Func::new receives a Caller, borrowed Val arguments and mutable Val results; its callback returns execution status | Separate argument access, result publication and execution failure. Its universal Val representation does not establish compact Kagari enum storage |
| [Wasmtime 32.0.0 rooting](https://github.com/bytecodealliance/wasmtime/blob/v32.0.0/crates/wasmtime/src/runtime/gc/enabled/rooting.rs) | Rooted, RootScope and ManuallyRooted | Explicit scoped protection and longer-lived host roots; ending a RootScope removes its roots. These external roots are not a substitute for tracing edges inside native collections |
| [Dart 3.6.2 embedding API](https://github.com/dart-lang/sdk/blob/3.6.2/runtime/include/dart_api.h) | Dart_GetNativeArgument, Dart_SetReturnValue, Dart_NewPersistentHandle and Dart_DeletePersistentHandle | Read arguments and publish a return value through the call context; explicitly retain objects beyond a local scope. Dart's object-handle model does not establish unboxed enum payloads or allocation-free handles |
| [Swift 6.0.3 value witnesses](https://github.com/swiftlang/swift/blob/swift-6.0.3-RELEASE/include/swift/ABI/ValueWitness.def) | initializeWithCopy, destroy, getEnumTagSinglePayload, getEnumTag, size and stride | Type metadata supplies operations for otherwise unknown value layouts. This is an internal runtime ABI, not a public embedding API; Swift's ARC operations cannot replace Kagari's GC tracing or provider/origin retention |

The proposal below combines the call/result and lifetime separation illustrated
by Wasmtime/Dart with metadata-directed value operations illustrated by Swift.
Kagari must still define and validate its own typed native storage tracing,
checked borrowing, reload provenance and failure-origin preservation. None of
these references proves that an unchecked layout-plus-pointer interface is safe.

### Proposed native value API

This is a concrete proposal for review, not implemented Rust APIs. Text sketches
describe signatures/ordering without promising that they compile against current
CallContext. Reuse TypeArgument/StorageType, ArgumentView, explicit root packs,
NativeStorage and checked payload access; do not create a second native registry.

#### Existing access and proposed additions

There is no unrestricted with_payload_edit capability. The older conceptual
name below is a proposal, not an alias for current payload operations.

| Current owner | Implemented capability | Boundary |
| --- | --- | --- |
| [NativeCall::with_payload](../crates/kagari-runtime/src/native/context.rs) | Shared payload access under a checked operation loan | Physical loans end before callback/reentry/GC; no unrestricted mutable traced data |
| [ManagedStorage access](../crates/kagari-runtime/src/native/payload/managed/access.rs) | Checked managed field operations and edit_data for fixed Rust data | Actual field publication/accounting and fixed-data editing proof remain mandatory |
| [GcObject provider kernel](gc-implementation-design.md) | Reviewed unsafe construction/mutation/scanning/charge | Collection storage/algorithms and safe authoring wrappers remain library/NC responsibilities |

EL01 must preserve these checks while changing enum transport/layout. The general
collection provider and broader safe builder design remain at NC/the GC follow-up
owner; the sketches below do not activate them or grant blanket mutable access.

#### Complete synchronous usage sketch

The following sketches form a target API, with ordinary typed callbacks adapted
to the same underlying entry as generic callbacks and native collection methods.
They describe proposed Rust signatures and usage, not a compiling example. New
methods below extend existing owners; their names are review candidates. Async
scheduling, external host borrows and module publication retain their current
contracts; a call-scoped value cannot escape into a suspended computation.

```text
type NativeEntry = dyn for<'call>
    Fn(&mut CallContext<'call>) -> NativeResult<()> + Send + Sync;

// Argument locators borrow the protected call, not the temporary &mut self loan.
// Their private construction records the call owner and exact scoped type.
impl<'call> CallContext<'call> {
    fn argument(&self, index: usize) -> NativeResult<CallValue<'call>>;
    fn decode<T: FromKagari>(&mut self, value: CallValue<'call>) -> NativeResult<T>;
    fn with_view<R>(
        &self,
        value: CallValue<'call>,
        read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
    ) -> NativeResult<R>;
    fn retain(&mut self, value: CallValue<'call>) -> NativeResult<ScriptValue>;
    fn import(&mut self, value: &ScriptValue) -> NativeResult<CallValue<'call>>;
    fn result(&mut self) -> ResultWriter<'_, 'call>;

    fn with_payload_edit<S: NativePayload, R>(
        &mut self,
        receiver: CallValue<'call>,
        edit: impl FnOnce(&mut S, &mut StorageAccess<'_, 'call>) -> NativeResult<R>,
    ) -> NativeResult<R>;
}

impl<'access, 'call> ResultWriter<'access, 'call> {
    fn write<T: IntoKagari>(self, value: T) -> NativeResult<()>;
    fn copy_from(self, value: CallValue<'call>) -> NativeResult<()>;
    fn construct(self, member: &EnumMember, fields: &[CallValue<'call>])
        -> NativeResult<()>;
}
```

CallValue identifies a protected slot; it contains no public byte pointer and
needs no independent root. The lifetime must not be tied to the short borrow of
CallContext used to acquire it: reading an argument and subsequently mutating the
context to fill a result is an intended use. with_view instead opens a short
storage loan. Its callback may return owned scalar/inspection data, never a view
or a runtime operation that collects, reenters or changes the borrowed storage.
decode returns an owned Rust conversion or an explicitly retained handle.
import validates runtime/type ownership and establishes call-owned protection
before returning; dropping the input ScriptValue must not invalidate that locator.

First, register an ordinary Option callback and a generic identity callback in
the same module. The provider catalog contains the installed standard types.
Existing builder/declaration methods and explicit low-level Codec checks remain
the registration path; only the low-level callback's result contract changes.

```text
fn identity(cx: &mut CallContext<'_>) -> NativeResult<()> {
    let value = cx.argument(0)?;
    cx.result().copy_from(value)
}

fn host_module(providers: &DeclarationCatalog) -> NativeResult<NativeModule> {
    let mut module = ModuleBuilder::new("game::host", providers)?;
    let optional = Option::<u64>::kagari_type(providers)?;
    let echo = module.define_function(
        FunctionDecl::new("echo")
            .parameter("value", optional.clone())
            .returns(optional),
    )?;
    module.bind(
        echo,
        |_cx: &mut NativeContext<'_>, (value,): (Option<u64>,)|
            -> NativeResult<Option<u64>> { Ok(value) },
    )?;

    let id = module.define_function(FunctionDecl::new("identity"))?;
    module.function(&id, |function| {
        let item = function.type_parameter("T")?.ty();
        function.parameter("value", item.clone())?;
        function.returns(item)
    })?;
    module.bind(
        id,
        NativeBinding::new(vec![Codec::Value], Codec::Value, identity),
    )?;
    module.finish()
}
```

The echo adapter decodes Rust Option<u64>, calls the closure and writes its
returned value through ResultWriter::write. The identity binding forwards the
checked source through ResultWriter::copy_from without decoding T or creating a
ScriptValue root. Both enter through NativeEntry; native success requires a
complete result. Explicit low-level Unit callbacks call result().write(()).
An ordinary typed callback's outer NativeResult remains execution status;
NativeResult<Result<T,E>> has a separate inner script Result value.

For example, a script using the registered module can expose:

```text
use game::host;

pub struct Player {
    pub val id: u64,
}

pub fn echo_number(value: Option<u64>) -> Option<u64> {
    host::echo(value)
}

pub fn make() -> Option<Player> {
    Some(Player { id: 7 })
}

pub fn roundtrip(value: Option<Player>) -> Option<Player> {
    host::identity(value)
}
```

After installing host_module through the existing engine/provider setup and
loading the script, Rust uses the existing SDK facade in both directions:

```text
let root = program.root();
let echo = runtime.function::<(Option<u64>,), Option<u64>>(&root, "echo_number")?;
assert_eq!(runtime.call(&echo, (Some(7),))?, Some(7));

let make = runtime.function::<(), Option<Object>>(&root, "make")?;
let roundtrip = runtime.function::<(Option<Object>,), Option<Object>>(
    &root, "roundtrip",
)?;
let saved: Option<Object> = runtime.call(&make, ())?;
let returned = runtime.call(&roundtrip, (saved.clone(),))?;
drop(returned);
drop(saved);
```

Object is the existing checked retained script-object handle. Keeping saved
across calls protects the Player through that handle, without a script Option
wrapper. For an unknown nominal value, use the existing ScriptValue mapping
instead. A low-level callback can explicitly retain a complete generic value:

```text
let value = cx.argument(0)?;
let saved: ScriptValue = cx.retain(value)?;
// Rust may move saved into host-owned state after this native call ends.
// In a later call on the same runtime:
let restored = cx.import(&saved)?;
cx.result().copy_from(restored)?;
```

The two contexts in this last sketch belong to separate calls. Retained values
keep exact type/provider dependencies and reachable script values alive, not the
Runtime itself or authority after shutdown. Typed Object decoding may incur
explicit host-root costs; low-level copy_from does not need such a conversion.

The following collection example additionally assumes the proposed general
provider edit boundary, which is not implemented. It illustrates the target API
after that boundary is established, not an immediately usable GC call or an EL01
requirement to migrate standard collection ownership. StoredValues
holds one shared applied type/layout and initialized records; there is no Rust T
parameter because each script instantiation supplies its type at runtime.
StorageContext::values creates an empty, non-rooting store under the existing
native payload construction owner. Its charge reports logical elements and
exclusively owned backing capacity; Runtime adds the enclosing payload size.

```text
impl StorageContext<'_> {
    fn values(&self, element: TypeArgument) -> NativeResult<StoredValues>;
}

impl<'access, 'call> StorageAccess<'access, 'call> {
    fn reserve(&mut self, values: &mut StoredValues, additional: usize)
        -> NativeResult<()>;
    fn push(&mut self, values: &mut StoredValues, source: CallValue<'call>)
        -> NativeResult<()>;
    fn commit_structural_change(&mut self);
}

#[derive(Debug)]
struct VecPayload {
    elements: StoredValues,
}

impl VecPayload {
    fn new(cx: &StorageContext<'_>) -> NativeResult<Self> {
        Ok(Self {
            elements: cx.values(cx.type_parameter(0)?)?,
        })
    }
}

impl NativePayload for VecPayload {
    fn trace(&self, visit: &mut ValueTracer<'_>) {
        visit.values(&self.elements);
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        self.elements.charge()
    }
}

fn vec_push(cx: &mut CallContext<'_>) -> NativeResult<()> {
    let receiver = cx.argument(0)?;
    let value = cx.argument(1)?;
    cx.with_payload_edit::<VecPayload, _>(receiver, |vec, access| {
        access.reserve(&mut vec.elements, 1)?;
        access.push(&mut vec.elements, value)?;
        access.commit_structural_change();
        Ok(())
    })?;
    cx.result().write(())
}
```

The existing Vec declaration/storage registration attaches VecPayload::new as
its factory and vec_push to its authored push method through the existing bind
path. The sketch changes the value/trace contract; it does not invent a second
type registration mechanism or replace the Vec declaration with an opaque type.
For an application-defined opaque container, NativeStorage::new(VecPayload::new)
uses the existing factory entrypoint, subject to its declared storage contract.

The proposed with_payload_edit would check receiver type/access, establish a
checked mutable payload loan and supply owner-bound StorageAccess. Existing
shared payload access alone cannot provide this capability. All GC preparation occurs before this
loan. reserve admits non-collecting backing growth and updates accounting; push
validates source type/layout and capacity before its short non-failing publication
of bytes/initialization/edges. The library explicitly identifies structural change;
Runtime owns its revision. Earlier successful operations are not rolled back if
a later operation fails. The shown push has one publication, followed by a
non-failing structural notification and its already checked Unit result write.

For Vec<Option<Player>>, the store's type is Option<Player>. push copies the
encoded optional record and Player handle where active. VecPayload has no
Some/None, object/scalar or niche switches. ValueTracer scans initialized records
through their shared layout; collection elements do not become external roots.
Primitive storage specializations remain permitted. Read/remove operations must
prepare a protected call destination before borrowing/removing a record; they
cannot return an element pointer. HashMap uses separate key/value stores under
this same access owner, as detailed in the following walkthrough.

The payload-edit signatures are proposed extensions of checked access, not a
claim that arbitrary handwritten tracing providers already receive safe mutable
access. The native collection plan still owns provider integration classification
and registration proof. EL01 must establish that boundary before exposing these
operations; the safe call/value APIs cannot bypass it.

#### Representation and lifetime obligations

| Proposed surface | Owner, data and permitted lifetime |
| --- | --- |
| Prepared element/value layout | Existing checked type scope extended with representation/scan operations; shared by slots/storage and retaining exact supplying provenance |
| CallValue<'call> | Opaque admitted location in an already rooted argument or temporary pack; no owning byte pointer, no independent per-argument root |
| ValueView<'view> | Short descriptor-directed projection under a storage borrow; cannot escape or cross reentry, allocation, collection or growth |
| ResultWriter | The selected native call's prepared result destination; staged initialization becomes visible only on successful native return |
| Existing ScriptValue / RootedValue | Safe retained value and underlying runtime-owned typed root; may escape a synchronous call |
| StoredValues | Non-rooting typed backing records belonging to a traced Native payload or protected construction/edit scope; not an ordinary host-owned root handle |
| ValueTracer | Runtime visitor for initialized typed records and their descriptor/object/origin edges; invokes no script callbacks |

CallValue may survive a synchronous nested call because it identifies protected
caller storage rather than borrowing its bytes. Its owner/generation is checked
when projecting again. Reading a mutable collection element is different: copy
the selected value into a protected call temporary before ending the payload
loan, so later mutation cannot change an argument already evaluated. A raw
ValueView cannot be retained merely by copying a locator out of it.

Argument/result types come from the already linked scoped signature, including
generic substitutions. Member handles come from existing prepared enum/type
evidence, not per-call names or an Option-specific setter. Views expose checked
scalar reads and variant/field projections, not a safe raw-byte mutation API.
Conversion/copy uses admitted source/destination layouts and preserves ordinary
compatibility rules; numeric tag/descriptor equality alone is not type admission.

Generalize NativeEntry from returning NativeResult<Value> to returning execution
status NativeResult<()>. Typed Rust closure adapters still accept/return registered
Rust values; their generated adapter fills the same result destination. A script
Result::Err is constructed in that destination and is distinct from an outer
NativeResult error representing execution failure.

The writer starts uninitialized, validates complete type/variant/payload evidence,
and stages one fully initialized value. Successful return requires that result;
Unit adapters explicitly supply Unit and Never cannot return successfully. A
second initialization rejects. Failure/unwind discards staged result retention
without undoing already committed script effects. Partial child conversions use
existing conversion/root packs; the final record write is short and non-collecting.
GC never interprets a half-written tag/payload as a complete enum.

copy_from forwards all active origins. construct captures a fresh origin only
when the selected newly constructed variant is marked reports_failure. Standard
From/map_err/propagation adapters need an explicit validated origin-forwarding
construction path using the original carrier, preserving existing semantics;
this capability is shared by marked variants and is not Result-name recognition.

StoredValues provides slot reservation, initialized-record copy/removal and
descriptor-directed inspection, behind checked storage access. The interface
does not require a Vec<u8> backend: preserve primitive specializations and allow
packed/aligned records according to the selected layout. Physical capacity and
logical initialization are separate, including zero-byte elements and free hash
slots. A record itself does not own an external root.

These are generic storage operations, not sequence/map method names in Runtime.
StorageAccess validates type/owner/generation, accounts backing growth, maintains
initialization and publishes object/metadata edges at the current collector's
required boundary. The library owns bounds/order/hash algorithms and chooses
which mutation is structural, through the existing runtime-owned revision.
Any preparation that may collect occurs before taking the payload loan;
reserve inside the loan performs only admitted non-collecting backing growth.
The final commit must be prevalidated/non-failing after preparation; failures
before it leave semantic state unchanged. No callback or collection spans a
conflicting payload loan. Extract/copy-to-call operations establish temporary
protection before ending the source loan; removal cannot expose an unrooted value.

StoredValues may not be safely extracted from its traced owner and treated as
an independently rooted Rust value. Detached algorithms must keep it registered
as protected work state or retain required temporary values before a collecting
boundary. Any raw provider access retains the audited integration obligations of
the [native collection plan](native-collections-plan.md#consolidated-native-api-surface);
this proposal does not settle its queued unsafe-provider classification or
activate the collector/collection ownership migration. Ordinary Native callers
use retained handles and checked access rather than that raw provider boundary.

### HashMap insert/get walkthrough

A reference organization for a native payload is:

```text
MapPayload {
    keys: StoredValues,    // Retains the checked K type/layout.
    values: StoredValues,  // Retains the checked V type/layout.
    routing: Rust HashMap<script_hash, candidate EntryIds>,
}
```

K/V are runtime checked types here, not new Rust types generated for script
declarations. EntryId is a library-owned slot identity, never a pointer into a
resizable buffer or a script GC object. Occupancy keeps key/value records paired.
The routing shape illustrates reuse of cached hashes and collision buckets; it
does not select a new hash-table dependency, require rebuilding every builtin
key fast path, or move library hash algorithms into Runtime.

For insert(K,V):

1. Obtain the rooted caller arguments and the map's retained key/value contracts.
   Resolve checked Hash/Eq call routes through current-body selection.
2. Under the existing key-operation guard, hash the query outside payload loans.
   Copy candidate keys into protected temporary arguments, end each loan and
   perform Eq outside it. Reacquire/validate storage before accessing an entry.
3. Before changing state, prepare backing/index capacity, type admission,
   accounting and the staged Option<V> result containing the old value or None.
   Source and old value remain protected across all preparation/callbacks.
4. In a short non-collecting commit, publish the new/replacement records and
   routing/occupancy changes together, update the runtime revision when required,
   then complete the staged result. Trap/unwind cleanup preserves the specified
   completed effects and drops only temporary retention.

For get(K), perform the same guarded Hash/Eq selection, then borrow the matching
record only long enough to copy it into a prepared Some(V) result; a miss writes
the prepared None member. Return no borrowed bucket/element pointer. In
HashMap<u64, Option<Player>>, Some(V) can therefore be Some(None) or
Some(Some(player)); the outer miss remains distinct. ValueTracer visits occupied
key and value records and all their active metadata/origin edges, with no Hash/Eq
execution during collection. HashSet uses the same key side without value records.

This supplies one ordinary provider boundary for standard and application
containers. Standard-library ownership changes remain NC work; EL01 first
adapts current owners to these value/trace contracts and updates direct consumers.
No legacy NativeEntry/Value forwarding facade or per-element ScriptValue roots
should be kept as an alternative permanent enum path.

### Hash collections through the same boundary

HashMap retains separate checked K and V layouts; HashSet retains an element
layout. Hash routing/control data and stored key/value records are distinct
responsibilities. Records may occupy typed buckets or indexed backing storage;
this design does not select a replacement hash-table implementation. Preserve
the existing native-storage registration and checked key protocol preparation.

For `HashMap<u64, Option<Player>>`, a lookup hashes the scalar query, locates
candidate entries, compares keys through the selected Kagari Eq implementation
and copies the matching optional value into the prepared result. A missing key
returns outer None; a present key with stored None returns Some(None). The
representation preserves that distinction without rebuilding a heap Option.

Bytes alone do not establish key semantics: padding, inactive fields and error
origins must not participate in hashing/equality. Use the existing checked Hash/Eq
operations, including user code and current-body selection across compatible
reload. Cached stored hashes and the existing key-stability obligations remain;
do not invoke Rust byte equality as a substitute or add automatic rehash policy.

Release table/record borrows before invoking user Hash/Eq. Snapshot needed keys
into protected typed argument storage, execute the callbacks, then reacquire and
validate entry identities/revisions before committing, following the existing
lookup/mutation guards. Never retain a raw bucket pointer across reentry, growth
or collection. GC scans occupied records using both layouts, retaining active
object, descriptor and origin edges. This same provider-facing protocol must serve
stdlib and independently registered native containers; Hash storage cannot gain
a privileged bypass that Vec or another provider lacks.

## Roots, mutation, artifacts and reload

Runtime supplies complete payload and metadata edges to the collector. The
collector does not infer nominal enum policy or resolve variants. Decode the tag
before tracing; inactive union fields and unused niche payload bytes emit no edges.
Retain descriptor provenance even for None and payload-free variants, and retain
each active error record independently of script object edges.

Cover frames, argument/result packs, closures, cells, statics, nested enums,
tuples, containers, native payloads, host roots and parked computations. A write
validates the entire new value, then publishes tag/payload/origin state together
at the existing exclusive mutation boundary. Replacing a variant clears obsolete
edges and origins. Traps, cancellation, depth exhaustion and synchronous reentry
use existing cleanup paths; projections cannot outlive their protecting owner.

Source-free validation checks nominal identities, checked substitutions, variant
arity, `reports_failure`, encoding domains, niche reservations, bounded offsets,
heap recursion boundaries, payload access and trace maps. Runtime tokens and raw
Rust bytes never appear as portable evidence. A forged encoding annotation or
unchecked producer-supplied GC map is not proof. Backends consume the checked
layout, not a second local reconstruction of enum semantics.

Values retain supplying type/layout provenance through compatible publication.
Ordinary calls continue selecting current implementations; admitted frames and
created closures/Futures retain selected bodies/type environments under the
[activation contract](spec/module-activation.md#live-installations-and-call-selection).
Frozen layout checks remain mandatory. There is no live-value migration or
latest-name substitution. Releasing final values/roots/frames must allow unused
descriptors and origin records to be reclaimed.

Keep format/native ABI identifiers under the unpublished-development policy.
Invalidate changed development products/caches and regenerate affected fixtures
once the wire contract stabilizes. Remove replaced storage paths for inline
applications; heap fallback remains a supported layout for other applications,
not a compatibility reader or a value-dependent duplicate representation.

Target the current kagari-gc-backed Runtime [provider model](gc-implementation-design.md); fixed dynamic nullable-reference fields alone cannot implement
tag-dependent multi-edge scanning and origin retention. Transfer final adapter
requirements to GCR03 before cutover without expanding this work into replacing
the collector or claiming incremental integration has passed.

## Implementation sequence and build boundaries

EL00 is locally complete. EL01-EL04 are deferred and unstarted; production enum
storage is unchanged. The sequence below applies only after reactivation and
review against the Runtime execution model. Runtime/backend integration,
source-free validation and measured production gains remain outstanding.

| Phase | Work and required exit condition |
| --- | --- |
| EL00: model and baseline | Locally complete: existing-runtime measurements and finite layout/retention/span/copy models pass. Select 24-byte packed records, scoped typed views, eight-byte non-reused tokens and bounded recursive fallback; freeze niche and numeric acceptance rules below. This is model evidence, not integrated Runtime/backend acceptance. |
| EL01: coherent direct-layout cutover | Update contract/ABI facts, lowering, source-free validation, Runtime storage/roots/origins, stdlib/native codecs, interpreter and native/AOT consumers. Support mandatory Option/Result/user shapes with explicit tags and preserve all error origins. Remove superseded assumptions that every enum is a heap object. |
| EL02: compositional niche encoding | Implement checked bool/char/discriminant reservations and nested propagation through the same layout model. Keep direct encoding as the fallback and independent reference. Complete boundary rejection and actual native niche execution. This phase is required for delivery. |
| EL03: behavioral and cost acceptance | Complete selected generic/storage/GC/origin/reload/source-free/native regressions, batch artifact regeneration and paired measurements against EL00. Resolve carried failures and diagnose architectural regressions before adding further specializations. |
| EL04: documentation closure | Publish lasting contracts at architecture/spec/host/backend owners, transfer explicit remaining work and GCR obligations, then retire this design/plan. |

No intermediate broken-build allowance is declared. If EL01 needs a finite
non-building interval, list affected crates, exact errors and the owning repair
checkpoint here before starting. No production stubs or disabled validation.
EL01 alone does not complete the feature; EL02 and EL03 remain required.

## Validation and measured acceptance

Inspect and strengthen existing owners before adding cases. Start with
[Option/Result propagation](../crates/kagari/src/runtime/tests/language/result_option.rs),
[enum provenance](../crates/kagari/src/runtime/tests/language/enum_payloads.rs),
[error-origin tests](../crates/kagari/src/runtime/tests/embedding/error_traces.rs),
native variant conversion and execution-root tests. Fixed complete programs
belong in owning `.kgr` fixtures; structural/lifetime oracles stay explicit in Rust.

Mandatory inline witnesses include Option<u64>, Result<u64,u64> on both branches,
an object-bearing Option/Result, equivalent user enum shapes, Ordering, ControlFlow
and bounded nested/mixed Option/Result applications. Mandatory niche witnesses
include bool and Unicode scalar boundaries, a user enum's unused discriminants,
nested reuse and niche exhaustion into a valid direct layout. Tests must inspect
real selected encoding and allocation behavior, not merely successful execution.

Retain distinct owner-level contracts for:

- Complete scalar domains, no numeric sentinel collisions, nested branch identity,
  uninhabited residuals and precise variant payload projections.
- Multiple active fields/edges, heap fallback for oversized/recursive layouts,
  checked size/alignment arithmetic and initialized/inactive storage.
- Once-only effects and generic Try/FromResidual/From propagation; fresh versus
  forwarded/converted Err origins, nested origins, and origin-ignored Eq/Hash.
- Shared aliasing, generic calls, fields/collections, Rust DTO conversion and
  representation-independent reflection/debug views.
- Foreign/stale descriptor, object and origin-token rejection, including payload-
  free variants; malformed source-free facts reaching the intended validator.
- Collection/reentry during live payloads, variant overwrite, parked/resumed
  computations and trap/cancellation/depth cleanup. Assert object liveness and
  complete descriptor/origin/root reclamation, not just successful calls.
- Compatible reload preserving provenance/current inner calls, and rejection of
  incompatible frozen layouts or foreign installations.

Keep detailed domain/encoding matrices at contract/Runtime owners. Sample SDK
integration through serialized source-free artifacts, interpreter and actual
native direct/niche paths. Include an AOT ABI case in its platform CI owner.
Do not multiply every scalar by every backend and lifecycle locally.

Separate wrapper allocation counts, origin/payload allocations, allocated/retained
bytes, GC time, copy/dispatch cost, preparation and execution latency. Warm
descriptors separately and precreate payloads for wrapper counts; keep real GC
enabled for end-to-end workloads. Cover scalar iteration, Ok-heavy calls, Err
capture versus forwarding, object variants, nested/user enums and stored enum
collections. Controls include no-enum scalar/object/container code, direct-tag
encoding versus niche encoding and large/recursive heap cases. Reduced compact
element bytes must be established separately from fixed-size Value transport.

Required outcomes are zero per-value enum-wrapper allocations for admitted
inline routes, exercised niche compression/propagation, preserved origin behavior,
intact scalar specialization, complete reclamation and checked source-free/native
support. Not every Result application or complete `?` pipeline is promised to be
allocation-free: origin capture, payloads, explicit boxes and heap fallback remain
attributed costs. Mandatory witnesses cannot be removed to make counts pass.

The EL00 section below freezes numeric throughput/latency/memory gates. A uniform
wider Value was rejected; typed storage must pass no-enum and enum-heavy gates.
If those fail, diagnose the transport/layout architecture before local tuning.
Record revision, machine/toolchain, workspace profile/features/default parallelism,
cache state, workload dimensions and reproducible commands. Temporary probes go
under ignored `target/`; retain conclusions and limitations here. The results
below measure baseline/model costs; no production speedup or integrated layout
support is claimed by this branch.

Implementation checkpoints require selected affected tests plus structure,
formatting, workspace all-target Clippy and diff checks. Full-workspace tests
and exhaustive feature/backend/platform matrices remain CI-only. Design
document changes require content/link/diff checks. Added Rust measurement/model
code also requires focused runs, formatting, structure and Clippy checks.

## EL00 evidence and frozen acceptance gates

The [baseline example](../crates/kagari/examples/enum_layout_baseline/main.rs)
loads the fixed [measurement program](../crates/kagari/tests/fixtures/measurements/enum-layout-baseline.kgr).
Its `model` mode owns finite layout, descriptor/origin-retention, typed-span and
copy-cost probes. Those models do not participate in production execution and
must be retired at EL01 after useful independent oracles move to their owners.
They are not a second implementation or evidence of integrated backend/GC safety.

### Reproduction and measurement boundary

On baseline `7837fed4` plus this measurement harness: Windows 11 Pro
10.0.26300, Intel i9-12900K (16 cores/24 logical processors), x86_64 MSVC,
rustc 1.99.1 (`58cf8f9c3`, LLVM 23.1.1), Cargo 1.99.1. Use the workspace release
profile, default parallelism and no affinity changes. Execution is interpreter-only;
the diagnostics feature installs the existing per-thread counting allocator, so
timings include its bookkeeping. The initial release build used a cold product
cache; compilation is excluded from the reported measurements. Before each
measured case, construct/load a fresh Runtime and execute/collect twice to warm
its code, type and storage capacities. Inputs are supplied through typed host
arguments, not compiled-in loop counts.

```text
cargo run --release -p kagari --no-default-features --features source,kagari-runtime/execution-diagnostics --example enum_layout_baseline -- model
cargo run --release -p kagari --no-default-features --features source,kagari-runtime/execution-diagnostics --example enum_layout_baseline -- 1024 7 off
cargo run --release -p kagari --no-default-features --features source,kagari-runtime/execution-diagnostics --example enum_layout_baseline -- 1024 7 on
```

Run these sequentially, without concurrent builds/benchmarks. The second command
disables automatic GC for allocation attribution and collects explicitly after
each sample. The third uses the ordinary automatic policy for end-to-end timing.
Execution timers include host admission/result rooting but exclude result checking,
printing and explicit collection. Source-to-artifact/prepared-program times were
742.0 ms and 729.3 ms respectively. Engine setup was 454.5/450.8 ms, and fresh
Runtime construction/load medians were 28.119/27.425 ms across seven installations
after a discarded first installation. Engine setup and Runtime load are outside
the source/preparation and execution timers. No compile-latency improvement is claimed.

GC object allocations are derived from the change in
`allocated_objects + reclaimed_objects`: the first counter alone is live
occupancy and falls during collection. Rust allocator requests include payload
vectors, metadata work, origins and GC machinery, not just object allocations.
Retained numbers below are allocator-requested net deltas across execution plus
explicit collection with the result rooted; they exclude warmed retained capacity,
allocator overhead and RSS. The runtime's partial retained-byte ledger alone
does not account for these objects/origins and is not used as total memory.
All sampled cases return to zero net requested-byte delta after dropping results
and collecting; explicit host root counts also return to zero.

### Existing-runtime baseline

Medians of seven samples, 1,024 iterations per case. GC object counts and Rust
request counts in the table come from automatic-GC-off runs. The two time columns
are distinct workloads with and without automatic collection, not a speedup ratio.

| Case | GC objects | Rust requests | GC off, ms | GC on, ms |
| --- | ---: | ---: | ---: | ---: |
| Scalar through shared `identity<T>` | 0 | 45 | 0.601 | 0.633 |
| One shared Player, repeated reads | 1 | 51 | 0.664 | 0.640 |
| Some(u64) | 1,024 | 1,078 | 1.378 | 1.523 |
| None | 1,024 | 54 | 1.323 | 1.377 |
| Ok(u64) | 1,024 | 1,078 | 1.454 | 1.445 |
| Fresh Err(u64) | 1,024 | 5,174 | 1.889 | 1.948 |
| Reuse one Err through `identity<T>` | 1 | 51 | 1.313 | 1.270 |
| Reuse one Err through a function using ? | 5,121 | 48,355 | 9.781 | 10.878 |
| Some(shared Player) | 1,025 | 1,084 | 1.694 | 1.721 |
| Same-shape user `Lookup<u64>` | 1,024 | 1,078 | 1.404 | 1.561 |
| Some(Some(u64)) | 2,048 | 5,175 | 2.385 | 2.569 |

Fresh Err adds 4,096 requests over Ok in this fixture, attributable to its bounded
origin capture path. Reusing the same Err through identity does not repeat that
capture. The `?` case constructs protocol/residual/forwarded enum values: its
5,121 objects are one seed plus five per iteration. Its much larger request count
also includes protocol preparation/admission work; wrapper removal alone is not
evidence that the whole `?` path becomes allocation-free or reaches scalar speed.
The default-GC run performs ten collections in this case. No scalar/enum time
comparison here isolates the cost of only a discriminant or only one allocation.

| Returned storage, 1,024 elements | Rooted net requested-byte delta | GC objects allocated |
| --- | ---: | ---: |
| `Vec<u64>` | 9,912 | 1 |
| `Vec<Player>`, one shared Player | 18,688 | 2 |
| `Vec<Option<u64>>` | 174,880 | 1,025 |
| `Vec<Result<u64,u64>>`, all Ok | 175,624 | 1,025 |
| `Vec<Result<u64,u64>>`, all fresh Err | 918,024 | 1,025 |

These are phase deltas, including result retention and metadata touched by the
path, not per-object malloc sizes. The object/enum programs deliberately retain
different payloads. Err's additional memory must remain separately attributed;
it cannot be counted as eliminated by removing the enum wrapper.

### Model outcomes and selection

Packed model sizes, excluding the shared applied descriptor: Option<bool> is one
byte, Option<Option<bool>> one byte, Option<char> four, Option<u64> nine,
Option<object> thirteen, Result<u64,u64> seventeen including an eight-byte active
origin slot, Option<Result<u64,u64>> seventeen and Result<Result<u64,u64>,u64>
eighteen. A single variant with two full managed handles fits 24 bytes. A tested
two-active-origin u64 shape needs 26 bytes and takes heap fallback under the frozen
budget. These are model results, not current Runtime layouts or Rust ABI guarantees.

The niche model checks every encoded byte of nested optional bool, 254 layers of
single-byte niche consumption, fallback to direct encoding after exhaustion,
Unicode boundary validity and uninhabited variants. Its finite graph model puts
self/mutually recursive declarations and oversized records on the heap while
allowing their nonrecursive optional parents to inline. The reachability probe
is deliberately small; production requires one bounded dependency/SCC pass.

The retention model checks copied roots, two independently retained origins,
tag-dependent inactive edges, foreign/stale token rejection, validation before
sweep, payload-free provenance and reclamation after final roots disappear.
Its immutable records contain no script values. It does not prove real Runtime
reload compatibility, safepoint/reentry admission, or source-free rejection.
The typed-span probe copies 1,024 zero-, one-, nine- and seventeen-byte records
with zero allocator requests after capacity setup. It rejects incompatible
descriptor identities and out-of-bounds indices, including zero-sized storage.
Those scoped views are 24 bytes; existing non-enum Value stays 16 bytes. Real
Runtime view lifetimes and native result publication remain EL01 obligations.

The rejected uniform carrier would make persistent Value 40 bytes. For 262,144
entries in each of two buffers, the copy probe requests 8,388,608 bytes at stride
16 versus 20,971,520 at stride 40. Five samples of 64 full-buffer copies have
medians 7.163 ms and 21.289 ms respectively. This is a raw byte-copy workload,
not a VM performance prediction. It establishes a concrete storage penalty and
supports typed owners/views rather than widening every value.

The 24-byte inline budget bounds copies while covering the mandatory scalar,
single-origin nested and two-handle witnesses. It is a finite design choice,
not a claim that 24 is globally optimal. Origin/descriptor tokens use checked
monotonic IDs with owner-table admission and tracing, rather than narrowing
existing generational handles. No managed-handle niche is assumed.

### Gates for EL01-EL03

These are acceptance targets, not achieved production improvements. Compare the
same harness before/after, on the same machine/profile/features, alternating
runs, with at least seven warmed samples and two independent paired runs. When
timing noise prevents a decision, repeat the affected pair and report its spread.

- Preserve 16-byte non-enum Value and existing primitive/managed element strides.
  No per-enum side allocation, per-copy origin allocation or global metadata leak.
- Zero wrapper-allocation slope for admitted inline shapes at two input sizes;
  separate fresh-origin, payload, explicit-box and backing-capacity allocations.
- Exercise the mandatory bool/char/discriminant niches and nested propagation.
  Their model sizes above are compact-record gates, not generic slot-size claims.
- At least 15% lower median execution time for Some, Ok, user Lookup and nested
  Option cases in both GC modes; at least 10% for the existing `?` forwarding case.
  Fresh Err and reused Err paths may regress at most 5%, with origin semantics intact.
- No-enum scalar, object-alias and scalar/object storage medians may regress at
  most 5%; their rooted net requested-byte deltas may grow at most 5%. Cold
  source/preparation and Runtime-load measurements must be separated; cap their
  paired regression at 10%, rather than hiding preparation in warm timing.
- At least 50% lower rooted net requested-byte delta for Option/Ok collections,
  and 10% for fresh-Err collections, preserving origin records. All retired
  samples return to zero net phase delta/root counts as in the baseline.

Failure of a gate requires ownership/layout diagnosis before further local tuning.
No gate permits lost nested states/origins, weaker identity checks or permanently
broken backends. The model can establish a candidate, not pass these production gates.

### Local validation and remaining scope

Both final existing-runtime runs pass all 16 workloads with seven measured samples
per workload (automatic GC off/on), including checksum or stored-length assertions,
zero final roots and zero net requested-byte phase deltas after retirement.
The release `model` run passes its layout/domain, SCC/fallback, lifetime/rejection,
zero-sized span and zero-allocation copy assertions. They cover the added probe
and design choices without rerunning unrelated language/backend tests.

`cargo clippy --workspace --all-targets -- -D warnings`, the feature-enabled
example's scoped Clippy, `uv run --locked scripts/check_structure.py` (1,863 Rust
files, zero violations), workspace formatting, fixture formatting and diff checks
pass. The scoped Clippy command uses the same features/example as the reproduction
commands with `cargo clippy ... -- -D warnings`; default workspace Clippy alone
does not enable this diagnostics-gated example. No checker changes or exceptions
were added. No current build/validation failure is carried.

Initial probe setup used EngineConfig's wrong field and the low-level zero-argument
execution entry; those failed before measurements and were corrected to
`default_runtime.gc` and checked typed invocation. Counter review also replaced
live occupancy with allocated-plus-reclaimed deltas before the final GC-on run.
No unsupported entry or partial counter result is used in the tables.

Full-workspace tests and exhaustive features/backends/platforms remain unrun
locally and CI-owned. Source-free enum-layout rejection, actual native/AOT layout
execution, real Runtime metadata/GC/reentry integration and production performance
gates remain EL01-EL03 work. Value structs and collector replacement remain deferred.
