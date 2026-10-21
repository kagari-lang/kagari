# Kagari Bytecode Artifact Specification

This document defines the `.kbc` artifact boundary.
The semantic bytecode model is defined in [bytecode.md](bytecode.md).

## Design Goals

- support precompiled script distribution and faster loading
- make artifact compatibility explicit
- preserve enough metadata for verification, diagnostics, hot reload, GC, and JIT
- keep the binary encoding versioned separately from language semantics
- reject stale or incompatible artifacts before execution

## Artifact Role

A `.kbc` artifact is precompiled Kagari bytecode plus metadata.
It is not native code. [AOT bundles](aot.md) pair verified bytecode with a separately
trusted native image and deployment manifest.
It is not trusted without validation.

Artifacts may be used by:

- CLI execution
- embedded hosts
- build pipelines
- hot reload systems
- package distribution
- cache directories

## Current executable contract

Current products use KBC format v115, `kagari-runtime-abi-v138`, `KMIR` v13
and runtime-helper ABI v6. Native bindings use module-qualified declaration IDs;
the provider descriptor and its separate per-contract version are removed. Unsupported versioned products are rejected before execution without a migration
reader. Unpublished schema changes below retain these identifiers, not old readers.

Integer identities, signatures and scalar layouts use only fixed-width i8/i16/i32/i64
and u8/u16/u32/u64. Standard lengths, indices and logical counts use u64. Products
encode these language contracts, not Rust pointer-sized slice layouts; no removed
machine-named integer identity has an alias or compatibility decoder. This
unpublished migration keeps format/ABI identifiers unchanged. Regenerate affected
development artifacts and caches instead of loading their obsolete schema.

Ordinary nominal enum nodes and layouts now cover source, standard and registered
native enums. Layouts carry full declaration/member identities, applied arguments,
ordered payload types and the explicit variant reporting fact. Generic enum
instructions validate those layouts. Library-authored native methods transfer
optional diagnostic metadata between checked live enum values. Linked validation checks nominal
kind and arity from declarations or validated private layouts. Fixed standard tags,
standard type nodes and MapResultError are removed. This unpublished replacement
keeps the current format/ABI identifiers; affected development fixtures are
regenerated at the final coherent checkpoint, with no previous-schema reader.
Try/FromResidual lowering contributes ordinary checked calls and ControlFlow
operations. Native Result conversion includes a bound From callback; implicit
identity/lossless adapters carry their source interface as part of the instance
identity. Selected or forwarded static trait members retain checked signatures,
scopes and supplying generations. The loader validates those proofs and installed
contracts without rerunning propagation inference.
Shared constraint calls may retain abstract associated outputs when the caller
provides verified scope and bound evidence. Boxed interface calls require complete
outputs and an instance member. Static constraint members use their declared
parameters without an invented receiver value. Runtime associated projections
read the pinned implementation table of the already selected callable; immutable
type scopes preserve those output facts and their layout owners through collection
and reload without retaining unrelated executable operation groups.
Historical milestones below describe earlier development snapshots.

One native import table carries source identity, concrete type arguments, binding
ID, applied signature, declaration bounds and ordered selected callable applications.
Each application carries the required receiver/interface/member, concrete target
identity and type arguments, implementation kind, full signature and conservative
effects. Offline verification reselects against carried checked implementation
facts and rejects a mismatched or missing executable target. MIR modules carry
body-free native target applications for concrete interface slots and selected
native dependencies; the source-free bytecode lowerer copies these checked
contracts rather than reconstructing native method applications. The runtime invokes
the sealed selection through its retained dependency generation. Generic verification
checks carried source declarations and instantiation. Native effects use a common conservative
classification, independent of artifact claims. Runtime resolves the installed ID,
checks the application against its trusted registered declaration and pins the
entry owner. No per-function effect/access table or duplicate signature template is
serialized. Native API packages install ordinary declaration records directly alongside Rust
handlers. The minimal route has no standalone binary declaration payload. Source
compilation must emit the same contracts as the registered definitions.

Synchronous host calls use this same import table and invocation driver. Manual
external-ID passing and effect policies belong to the common FnDecl and must
match installation exactly. Shared external schemas and explicit manual path
requirements remain independently validated. Source declarations, layouts,
interfaces and private dependencies accompany the program; executable contracts
have no source-analysis dependency.

Callable method policy remains independent of binding identity. Final methods
cannot be replaced; required and non-trait declarations cannot be final.

Interface method slots distinguish checked script function references from native
import references. The native import must match the exact implementation member
and applied trait signature. Both callable targets retain their implementation
generation and share frame, callback-return and cleanup validation.

Interface construction records carry checked construction identities, applied
arguments, parent views and result adapters. They grant no receiver comparison
capability and carry no comparison-selection certificates. Dynamic admission
validates the complete applied member/parent surface, including uncalled members.

Bound construction instructions and call records also retain declaration-derived
capabilities independently of method slots. Selected implementations carry their
operation and construction dependencies; forwarding requires the caller's bounds.
Both evidence kinds use one flat heterogeneous wire tree, checked for node kind,
4096-node and 64-depth limits before recursive executable objects are built.
Artifact-wide accounting traverses both kinds through the same child-edge owner.
Source-free verification checks exact receiver/target scope and selection coverage;
a retained implementation inventory is not proof of an otherwise absent bound.


Independent MIR and bytecode validation require known object operand types for
identity instructions. Primitive equality/hash instructions must prove eligibility
from exact semantic operand types and cannot bypass an explicit implementation.
Selected ordinary calls retain their supplying generations, roots and cleanup
contracts without runtime source resolution. See
[the architecture owner](../architecture.md#library-identities-and-representation-boundaries).

Trait declarations no longer encode collection storage markers. Exact installed
contracts, conversion-adapter authority, host storage access and operation witnesses
retain their independent validators. The unpublished schema replacement keeps
existing format/ABI identifiers and regenerates disposable fixtures without an
old-schema reader. Generated adapter identity arguments encode the applied
protocol as declaration metadata, independently of any erased interface value view.

Invocation state owns Rust data and explicit roots. Native callbacks execute
synchronously on shared frames; results remain rooted across allocation/reentry.
Cooperative cancellation and individual mutation commit rules remain explicit.
Native payloads trace retained script values/callables. Artifacts contain portable
contracts, not live heap state, Rust references or function pointers.

### Unified executable slots and origins

Current function records encode `slot_count: u32`, one `slots` representation vector,
explicit `parameter_slots` and a separate `BytecodeSemanticSlots` projection. `Slot`
is a checked u32 identity, not either half of a local/register split. Counts and
vectors are bounded before decoding/verification; parameter homes must be distinct,
in range and compatible with physical and retained semantic signatures. Entry
initializes only those homes. Bounded CFG verification checks initialized reads and
semantic/access facts through writes, joins and loops for ordinary as well as
suspendable functions. Full nominal/reference/generic/protocol/numeric facts remain
independent of shared physical representations; missing facts are not unchecked Any.
MIR retains its own semantic locals/temporaries and is not the bytecode slot schema.

Canonical instructions include `BinaryOperands` and `PredicateBranch` with
`ScalarOperand::Slot` or `Constant { constant: ConstantId, ty: BuiltinType }`.
Constants reference the canonical pool and preserve exact numeric width/range and
float bits. Predicate records retain the original comparison, explicit `negated`
Boolean polarity and both final-PC targets; polarity is not ordered-predicate
inversion. Ordinary comparison protocol and object-identity restrictions apply.
No literal/immediate encoding rule or second numeric implementation is introduced.

Mandatory `origins: Vec<InstructionOrigin>` retains semantic MIR emission ordinals
as origin IDs and maps them monotonically, without final-PC gaps, onto the final
instruction stream. Each record retains its own span and optional coordinates.
`instruction_origins` selects exactly one actual origin per ambiguous many-to-one
PC group, ordered by PC; unique-origin PCs need no override. Canonical encoding and
decoding validate this selection independently of optional debug data. Removed
transport origins are not executable PCs or implicit actual-origin choices.
Native safepoint/trap references use origin IDs, not final-PC bounds; source-free
[AOT loading](aot.md#bundle-and-trust-contract) checks them against this carried
mapping without MIR or source. The [bytecode contract](bytecode.md#executable-origins-and-final-pcs)
owns the detailed mapping rules.

`LocalLiveRange` carries source `binding` identity separately from `slot` and final-PC
availability. Runtime-only bank allocation and prepared scalar sites are derived
from verified code, never serialized live locations, heap handles or kernel pointers.
These implemented contracts are independent of blocked BO04 performance acceptance.
The unpublished replacement keeps identifiers unchanged and invalidates obsolete
artifacts/caches rather than adding an old-schema reader.

## Offline disassembly validation

[`kg disasm`](toolchain-tools.md#kbc-disassembly) and
`kagari_bytecode::disassembly::PreparedDisassembly::from_bytes` inspect current-format
KBC without source files, installed native implementations or a runtime. They reuse
the existing codec and verifier, not a permissive second decoder:

1. The CLI reads at most `MAX_ARTIFACT_BYTES` plus one sentinel byte and rejects
   oversized input. Decoding also enforces existing byte/count/depth limits.
2. `KbcArtifact::from_bytes` decodes records with their definition metadata.
   Scope-preserving import into an offline `DefinitionContext` remaps identities;
   numeric IDs are never interpreted in an unrelated table.
3. `KbcArtifact::into_verified` uses current `ArtifactCompatibility::default()`
   requirements. Header/version/ABI, content hash, envelope/table consistency and
   complete bytecode graph/type/reference/resource checks remain in force. A stored
   verification flag is not trusted. Compatibility is not derived from the input
   header merely to make it pass.
4. Only after full validation may listing or selection proceed. Invalid unselected
   functions still reject the artifact. There is no raw/unchecked fallback or
   older-format migration reader.

The summary reports encoded format/language/compiler/ABI provenance, content hash,
root module, module/function counts and `validation=envelope+bytecode`. The root
module is not an assertion about public exports or an entry function. The hash is
content identity, not authentication or a live runtime generation.

Optional portable MIR is opaque: presence is reported as `present, not semantically
checked`, and its buffer is released during preparation. Disassembly does not
establish MIR/bytecode correspondence, native eligibility, signature authentication
or host linkability. A valid custom-native contract can be inspected without its
Rust provider, but that does not establish that it can execute in a particular host.
See [library ownership](../architecture.md#offline-bytecode-inspection) and
[output semantics](bytecode.md#disassembly-output).

## SDK Feature Boundary

The SDK separates source compilation from native preparation. Its default feature
set enables both `source` and `native`; hosts may disable defaults and select:

| SDK features | Available preparation and execution | Production dependency boundary |
| --- | --- | --- |
| None | Validate/load/reload artifacts and interpret bytecode | No compiler, HIR, syntax, MIR, codegen or concrete backend |
| `source` | Source analysis and artifact emission, plus bytecode execution | Compiler source/HIR/syntax/MIR enabled; no codegen required |
| `native` | Decode verified portable MIR, compile through a trusted backend, install and execute | Compiler core/MIR/codegen enabled; no HIR or syntax |
| `source,native` | Both paths | Combination of the above |

The additive `web` feature supplies browser/Worker measurement clocks on
`wasm32-unknown-unknown`; it does not enable source compilation or native preparation.
That target requires `web` and a browser Performance clock. Other targets keep the
standard clock, without web-time dependencies. WASM targets reject `aot-load` and
`aot-build`, which require native dynamic-library products. Browser bindings and
execution acceptance are tracked separately in the roadmap. The
[browser adapter](../../crates/kagari-web/README.md) defaults to source support;
its no-default-features product omits the frontend and source compilation export.
Both use the same prepared-program validation and interpreter. Source inputs and
artifact bytes are bounded before admission; browser-owned results are converted
while rooted, without exposing heap addresses.

`PreparedProgram::from_artifact` always validates the envelope and bytecode. With
`native` enabled it additionally checks portable MIR and canonical correspondence;
without `native`, the MIR payload remains opaque and only its envelope bounds and
integrity are interpreted. A source-only build can still emit portable MIR for a
separate native-enabled consumer. Native backends are supplied by the host.

Loading and reload accept shared prepared programs. Runtime instances, host
bindings and installed native handles remain separate for each runtime. CLI source
and artifact execution prepare before linking; its `jit` feature enables native
SDK preparation and the concrete Cranelift dependency.

Reproduce feature isolation with `uv run python scripts/check_features.py`.
The script checks eight production crate graphs and builds standalone consumers
with no features, `source`, `native` and `source,native`, using the repository's
locked dependency versions and normal target directory. The native-only consumer
runs real Cranelift code from the tracked portable artifact, without script source
text or production HIR/syntax dependencies. The audit also checks that ABI has no
source-analysis dependencies through build edges. Workspace dev-feature unification
is not used as proof of isolation. Pending migrations and genuinely carried errors
belong to the [roadmap](../implementation-roadmap.md).

## Logical Layout

An artifact contains:

```text
KbcArtifact {
  header: ArtifactHeader,
  program: BytecodeProgram,
  tables: ArtifactTables,
  verification: VerificationMetadata,
  debug: DebugMetadata?,
  signatures: ArtifactSignatures?,
  portable_mir: PortableMir?
}
```

The current format identifiers are defined in
[artifact.rs](../../crates/kagari-bytecode/src/artifact.rs),
[version.rs](../../crates/kagari-abi/src/version.rs) and the
[MIR codec](../../crates/kagari-mir/src/codec.rs). They remain unchanged for
unpublished internal schema edits; development artifacts must be regenerated.
There is no compatibility reader for retired layouts.

KBC uses fixed-width integers, little-endian byte order and declaration-order
fields. It contains a complete ordered BytecodeProgram, root ModuleRef and
module/function call slots. Struct and enum layout tables retain nominal
identities, concrete arguments and field/variant contracts. Physical aggregate
instructions are checked against these layouts and their defining public ABI.
Public items, private trait contracts and native declarations use one ordinary
semantic type tree. Generic binders retain declaration owner and position;
foreign/unbound parameters, escaped Self, invalid constructor arities and
noncanonical bounds reject verification.

The codec stores a canonical exact definition table and local u32 references.
Process-local table discriminators never enter portable bytes. Only referenced
definitions and their ancestors are included; unrelated names and allocation
order do not change identity. Duplicate identities, invalid parents, cycles and
out-of-range references reject before adoption. Scope validation and semantic
verification remain separate; interning a name does not supply a declaration.

ABI types use bounded flat preorder nodes. Decoding checks at most 4,096 nodes
and depth 64 before rebuilding recursive types. Module, function and instruction
sequences preflight at 1,024, 65,536 and 1,000,000 records respectively. Module-owned
tables preflight at 1,000,000 records; nested members, parameters, path segments and
instruction operand vectors preflight at 4,096. Aggregate quotas also apply across
the program. Native declaration/import signatures and external type fields use
the same semantic shape guards. Attached and detached debug frame layouts are
bounded before validation, fingerprinting or encoding.

Construction measures canonical encoded size before verification and bounds the
completed artifact before fingerprinting. In-memory loading and encoding repeat
the 64 MiB size check; byte decoding checks the input length first. This bounds
strings and signature bytes without a conflicting second per-field limit.
`from_bytes` checks magic and current format before decoding and rejects trailing
bytes. Decode alone grants no execution trust: header, bytecode, dependency,
metadata and content-hash checks still run. Invalid in-memory records return
structured errors before serialization or hashing.

Native calls use ordinary NativeImportId slots and exact NativeDeclarations.
Concrete identities and generic/callable requirements must match the retained
provider closure. Implementation tables use unique local Impl identities and
trait-owned member IDs; output contracts, bounds and signatures are rechecked
against reachable public or private trait contracts. Explicit native impls and
script impls share selection and overlap rules. Only intrinsic protocol evidence
uses a bridge marker; it must forward to the exact checked protocol adapter.
Runtime linking pins checked callable owners to the loaded generation.

External TypeDeclarations retain the complete shared schema closure, including
annotation-only uses and executable semantic types. Ordinary DTO layouts cannot
satisfy external Rust storage requirements. Explicit HostPathDeclarations retain
field, index and virtual segments, access and schema epoch. Each physical path
record must match a resolved declared contract. Loading requires exactly one
installed matching descriptor and adapter, including unused requirements; it
checks dynamic arguments and write access before publication. Program-local path
slots do not select runtime descriptor IDs directly. Diagnostic labels are
excluded from path identity and cannot retarget pinned bindings.

PortableMir is an optional opaque byte payload covered by the content hash and
section table. Its declared length is bounded before element decoding and the
complete envelope must fit the same 64 MiB limit. The bytecode crate neither
depends on MIR nor interprets the payload. A MIR-aware consumer validates and
adopts it through the MIR codec and verifies the resulting executable lowering;
bytecode-only loading still validates its envelope integrity and limits.

## Header

The header records:

- magic bytes
- artifact format version
- Kagari language version
- compiler version or compiler fingerprint
- target runtime ABI version
- runtime helper ABI version
- endianness or canonical encoding marker
- module id
- module epoch expectation, if relevant
- artifact content hash

The loader must reject artifacts with unsupported magic, format version, language version, runtime ABI, or helper ABI.

## Tables

Artifact tables include:

- constant pool
- type table
- function table
- public item table
- module slot table
- path descriptor table
- interface table
- ordinary native imports/declarations, shared external schemas and explicit path requirements
- string table
- source file table
- debug name table

Hot execution paths use table ids, not repeated strings.
Strings remain available for diagnostics, debug metadata, and tooling.
Interface method slots are checked against their concrete function identities:
the impl and method type arguments must instantiate the declared signature to
the executable parameter and return layouts. A slot with a mismatched instance
is rejected before execution.
For an implementation of an imported trait, whole-program verification also
checks the interface table against the trait's public ABI in its dependency
module and requires that module to be reachable.
Cross-module calls to specialized generic implementation methods resolve by
declaration identity and concrete type arguments during program linking. The
emitted call targets a verified function slot in the defining module; executable
bytecode carries no unresolved source declaration lookup.

## Verification Metadata

Verification metadata includes:

- unified logical slot representations, full semantic projection and explicit parameter homes
- mandatory origin-to-final-PC mappings and actual-origin choices
- instruction effect metadata
- control-flow target metadata
- safepoint metadata
- GC root metadata
- typed path descriptor fingerprints
- public ABI fingerprints
- dependency fingerprints

The loader verifies every member even when metadata is present. Root-level function,
effect, control-flow, public-ABI and path summaries refer to the root ModuleRef;
dependency metadata remains owned by each BytecodeModule. The loader derives and
compares these summaries against the actual program. They cannot replace verification.
Root-map completeness and per-table/depth decoding quotas remain pending.

Dependency fingerprints carry ModuleIdentity and are derived from the canonical
bytes of every non-root module in program order. ArtifactBuildOptions has
no dependency-fingerprint input. ArtifactCompatibility may pin an exact dependency
set with Some; None omits that external pin but still requires metadata/payload
agreement. Runtime code fingerprints derive from BytecodeProgram for both source and artifact loads; the
artifact content hash separately covers packaging, headers and auxiliary metadata.

## Debug Metadata

Debug metadata is optional.

It may include:

- source spans
- source file names or source uris
- safe debug point tables
- function names
- local names
- parameter names
- captured binding names
- binding identities, logical slot mappings and half-open final-PC live ranges
- captured value metadata and copied debug frame layouts
- type names
- path names
- line tables

Debug metadata may be stripped from production artifacts.
Stripping debug metadata must not change execution semantics. Mandatory executable
origin records/choices, parameter homes, semantic contracts and root layouts are
not this optional section and cannot be discarded with names or breakpoint tables.
Physical scalar reuse respects retained binding availability but does not require
permanent debug storage; managed slots currently remain distinct.

## Signatures and Trust

Embeddings may require artifact signatures or hashes.

Signature policy is host-controlled.
The language runtime only requires that signature metadata, when present, be validated before loading the artifact as trusted cache content.

The host chooses whether unsigned artifacts are accepted.

## Compatibility Rules

An artifact is compatible only when all required versions and fingerprints match:

- artifact format version
- language version
- compiler compatibility version
- runtime ABI version
- runtime helper ABI version
- dependency module fingerprints
- public ABI fingerprints
- typed path descriptor fingerprints

Incompatible artifacts are rejected or ignored as stale cache entries.
They must not be partially loaded.

## Loading Flow

Artifact loading proceeds as:

```text
read bytes
  -> decode implemented artifact encoding
  -> validate header
  -> validate versions and hashes
  -> decode tables
  -> validate module identity
  -> verify bytecode
  -> validate common declarations, dependencies and path requirements
  -> register loaded module candidate
  -> publish only after module/reload validation succeeds
```

## Hot Reload

Reload candidates may come from `.kbc` artifacts.

The reload validator compares artifact metadata against the active module epoch and runtime state.
Failed artifact reload does not replace the active module.

## JIT Interaction

JIT artifacts are separate from `.kbc` artifacts.

A `.kbc` artifact may include metadata useful for JIT compilation, but it does not contain machine code.
Machine-code caching across process restarts is outside the baseline JIT scope.

## Acceptance Criteria

The artifact format is complete when:

- `.kbc` files have versioned headers and logical sections
- incompatible artifacts are rejected before execution
- bytecode verification runs before publication
- debug metadata can be preserved or stripped without semantic changes
- artifacts carry enough metadata for hot reload, GC safepoints, typed path validation, and JIT compilation
- source loading and artifact loading produce the same module identity model

Loading compares required common native declarations and external schema/path
contracts against installed modules before initialization or publication. Signature,
borrow, effect and storage differences reject. Unrelated installed modules do not
change the required contracts. Canonical code fingerprints cover the complete
BytecodeProgram, including these requirements; there is no second host aggregate
or host-only fingerprint summary.

## Associated type metadata

Format 43 introduced trait-owned associated member
identities, declaration bounds, impl output definitions, interface equality
bindings and projection templates. Generic bound targets are structural ABI
types, so projections have the same canonical identity as parameter bounds.
Associated binding maps are ordered by declaration identity; decoding rejects
duplicate or noncanonical keys and applies the existing bounded type node,
depth, identity and record budgets. Linking verifies the complete output schema
and bounds even when no trait method is executed. Format 45 retains this metadata;
all prior formats are rejected.

Version 71 and runtime ABI v71 include the expanded Iterator method contract.
Earlier products are rejected before execution.

Version 72/runtime ABI v72 include source-owned iterator method bounds and ordered extrema.

Version 73/runtime ABI v73 add dynamic inner iterator dependencies and retain
engine default method signatures only in their canonical standard trait contract.

Version 74/runtime ABI v74 add Sum/Product and an explicit f64 constant encoding.

Version 75/runtime ABI v75 define buffered Result/Option collection lifting.

Version 76/runtime ABI v76 finalize source-declared native collection protocols
and checked narrow-integer aggregation. Earlier products are rejected.

Version 77/runtime ABI v77 add unsigned 64-bit constants and execution values,
including the full u64 range. Numeric literals preserve their checked scalar
types and precision; unsuffixed floating-point literals now default to f64.
Earlier artifact versions are rejected before execution.

Version 78/runtime ABI v78 retain concrete integer width and signedness in numeric
operations, including compound typed-path operations. Verification checks operand
representations and operation arity before execution. Earlier artifacts are rejected.

Version 79/runtime ABI v79 add declaration-backed integer method bindings and
source-width checked compound operations. Numeric bindings include both their
operation and concrete receiver type; invalid bindings are rejected by verification.

Version 80/runtime ABI v80 add verified numeric conversion descriptors and the
standard TryFromIntError/Infallible enum identities. Checked conversion results
carry their concrete Result payload types. Earlier products are rejected.

Version 81/runtime ABI v81 add the verified repeat-array instruction and array bulk
replacement intrinsics. Repeat counts have the semantic type u64; writable target
access and element types are checked before execution. Older products are rejected.

Version 82/runtime ABI v82 add immutable range ABI descriptors and canonical
encoding, MakeRange/RangeBound instructions, the standard Bound enum, lazy range
iteration and the normalized ArrayCopyWithinBounds intrinsic. Public copy_within
calls must lower their RangeBounds protocol calls before bytecode verification.
Validators reject missing endpoints, invalid element types, mismatched Bound
payloads and read-only destinations before execution. Earlier products are rejected.

Version 83/runtime ABI v83 restrict repeat-array instructions to element types
without shared mutable identities. Source analysis and portable verification share
the structural type predicate. Vec::from_fn lowers to ordinary control
flow and closure calls; an unlowered factory intrinsic is rejected. Older products
are rejected rather than interpreted with changed repetition rules.

Version 84/runtime ABI v84 replace native public access types with source-declared
List/MutableList, Map/MutableMap and Set/MutableSet interfaces. Vec,
LinkedHashMap and LinkedHashSet are the concrete native classes. Interface tables
encode a validated native-bridge flag, and ArrayCopyFromStorage is the normalized
snapshot-copy intrinsic. Standard interface ancestry is linked without requiring
a separate executable std module. Public List annotations carry nominal interface
ABI rather than native array ABI. Earlier products are rejected before execution.

Version 85/runtime ABI v85 make Map keys/values/entries return List interface
snapshots. Checked lowering emits private MapKeysStorage/MapValuesStorage/
MapEntriesStorage operations followed by a verified List conversion. The public
snapshot bindings are rejected if left unlowered, and the storage operations carry
native array result types through access validation. Version 84 products are
rejected before execution; there is no compatibility decoder.

Version 86/runtime ABI v86 add the declared, string-constrained List and Iterator
join operations. They lower to ordinary traversal and the existing ArrayJoin storage
intrinsic. Runtime interface tables preserve declaration ordinals for omitted native
defaults using vacant bindings. Version 85 products are rejected before execution.

## String query intrinsics (v87)

KBC/runtime ABI v87 adds Unicode trimming, byte-offset search and prefix/suffix
stripping. Operand and destination representations are checked before execution.
Earlier artifacts are rejected without migration.

## Lazy string traversal (v88)

KBC/runtime ABI v88 adds typed string-iterator constructors and split-once
intrinsics. Constructors validate the argument tuple (including checked storage-size
limits) and produce Iter<String>. Unlowered source bindings and previous artifacts are
rejected. Iterator positions are committed only after result allocation succeeds.

## String transformations and typed traversal (v89)

KBC/runtime ABI v89 adds replacement, repetition, casing and boundary queries.
String-iterator constructors encode byte/scalar-index modes and their exact item
types, retaining validation before execution. Previous products are rejected.

## Enum combinations (v90)

KBC/runtime ABI v90 adds source-declared Option/Result combinations. They lower
into verified control flow, standard enum operations and ordinary closure calls;
unlowered bindings are rejected. Forwarded errors use the trace-preserving
MapResultError operation. Previous artifacts are rejected without migration.

## Explicit parsing (v91)

KBC/runtime ABI v91 adds ParseError, typed native parsers and radix parsing.
String.parse lowers to static FromStr dispatch; parser operands and target types
are checked before execution. Previous formats are rejected.

## Map snapshot interface methods (v92)

KBC/runtime ABI v92 records Map's standard snapshot defaults and the renamed
array copy operation. Snapshot results have readonly List interface tables.
Previous products are rejected.

## List queries (v93)

KBC/runtime ABI v93 includes source-declared List query defaults. Native witnesses
inherit their standard methods without requiring duplicate inherent declarations.
Queries lower to verified control flow and ordinary comparison dispatch. Previous
products are rejected.

## List mutation capabilities (v94)

KBC/runtime ABI v94 extends MutableList's required methods and adds verified
array mutation bindings. Extend source traversal is lowered before one storage
commit; internal storage arguments must have matching element types. Previous
products are rejected.

## Concrete collection capacity (v95)

KBC/runtime ABI v95 adds typed capacity constructors, queries and reservation
bindings for concrete collections. Reservations retain write/allocation effects;
previous products are rejected.

## Set interface algebra (v96)

KBC/runtime ABI v96 removes concrete-only set algebra intrinsics. Set relations
and algebra lower through Iterable and Set member identities, including custom
storage and read-only views. Result construction uses the selected Eq/Hash
protocol. Old artifacts are rejected before execution.

## Guarded collection callbacks (v97)

KBC/runtime ABI v97 adds frame-owned collection mutation guards and MutableMap
update members. Native callback writes, including value replacement, are rejected
through all aliases until preparation completes. Guards root their target and
release on frame cleanup. Source-only Map combinators cannot appear as executable
intrinsics. Custom MutableMap implementations must provide the same documented
failure guarantee. Earlier artifacts are rejected rather than adapted.

## Prepared collection replacement (v98)

KBC/runtime ABI v98 adds private prepared array replacement and retention commits.
User callbacks execute in ordinary frames under mutation guards. Commit performs
no script calls, checks structure guards and cancellation, and preserves identity while
updating revision and live heap units. Source callback intrinsics are rejected
unless lowered. Prior formats are rejected.

## List snapshot traversal and immediate range removal (v99)

KBC/runtime ABI v99 adds List window/chunk default identities and private range
removal preparation. At that checkpoint lazy steps were verified script-backed
iterators retaining source guards and producing readonly List interfaces. Current
artifacts use GC-owned native captures and checked native continuations instead.
Range removal prepares its readonly result before a typed storage commit, including any interface allocation.
Previous formats are rejected before execution.

## Resuming indexed adapters (v100)

KBC/runtime ABI v100 adds a typed private iterator-resume operation. Before a
window/chunk step read source slots, it validated retained native source revisions
and reestablished source guards. That internal resume/guard protocol is obsolete:
current cursors check revisions on reads without lifetime iteration leases.

KBC/runtime ABI v101 adds the standard `Fn` protocol identity and callable
specializations. Callable adapters use the existing closure instruction and
semantic signature validation, including capture and return contracts. Earlier
artifacts are rejected; no legacy callable dispatch or migration is provided.

## Async artifacts

The implemented [async executable contracts](../async-execution-design.md#executable-contracts)
define the checked handoff.

Async programs serialize validated factory/resume and await contracts, never live
execution IDs, completion endpoints or host payloads. Encoded programs enforce the
same cold creation, single-drive, output typing and suspension-resource rules as
source programs. Live suspended executions are not serializable artifacts.
