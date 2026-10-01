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
It is not native code.
It is not trusted without validation.

Artifacts may be used by:

- CLI execution
- embedded hosts
- build pipelines
- hot reload systems
- package distribution
- cache directories

## Current executable contract

Current products use KBC format v113, `kagari-runtime-abi-v136`, `KMIR` v11
and runtime-helper ABI v6. Native bindings use module-qualified declaration IDs;
the provider descriptor and its separate per-contract version are removed. Older
products are rejected before execution without a migration reader.

One native import table carries source identity, concrete type arguments, binding
ID, applied signature and declaration bounds. Generic verification checks carried
source declarations and instantiation. Native effects use a common conservative
classification, independent of artifact claims. Runtime resolves the installed ID,
checks the application against its trusted source-derived declaration and pins the
entry owner. No per-function effect/access table or duplicate signature template is
serialized. The generated standard declaration payload uses ordinary declaration
records exported from the source pipeline and is checked against source emission.

Synchronous host calls use the same import table and invocation driver. Their full
optional HostFunctionDeclaration must match the required HostInterface and installed
registration; permissions, passing styles, schemas and borrow checks remain.
Source declarations, layouts, interfaces and private dependencies accompany the
program. Executable contracts have no source-analysis dependency.

Callable method policy remains independent of binding identity. Final methods
cannot be replaced; required and non-trait declarations cannot be final.

Invocation state owns Rust data and explicit roots. Checked callbacks run on shared
frames; return values stay rooted while receive() runs. Work uses logical budget
safepoints and unit mutations retain separate publication. Artifacts contain no
live heap state/Rust references. Generic persistent traced state remains NR03 work.

The reset native_provider.kbc fixture covers array direct/interface/callback calls
with serialized and source-free execution. The old feature_artifact.kbc remains
tracked for NR04 restoration: its bytes/API are superseded and do not load under
v113. This does not reduce the required final feature matrix.

## SDK Feature Boundary

The SDK separates source compilation from native preparation. Its default feature
set enables both `source` and `native`; hosts may disable defaults and select:

| SDK features | Available preparation and execution | Production dependency boundary |
| --- | --- | --- |
| None | Validate/load/reload artifacts and interpret bytecode | No source stdlib, compiler, HIR, syntax, MIR, codegen or concrete backend |
| `source` | Source analysis and artifact emission, plus bytecode execution | Stdlib/compiler source/HIR/syntax/MIR enabled; no codegen required |
| `native` | Decode verified portable MIR, compile through a trusted backend, install and execute | Compiler core/MIR/codegen enabled; no stdlib, HIR or syntax |
| `source,native` | Both paths | Combination of the above |

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
is not used as proof of isolation. Current migration acceptance and carried build
failures are recorded in the [active plan](../stdlib-hir-refactor.md#progress-ledger).

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

Format version 110 uses `bincode` with fixed-width integers, little-endian byte order,
and declaration-order fields. Runtime path binding identity uses index and
virtual segment fingerprints from resolved contract fields. All earlier versions are rejected; no
migration or compatibility decoder exists. The format stores a complete
stable ordered BytecodeProgram, its
root ModuleRef, and module/function call slots. Structs use nominal layout tables,
positional initializers and layout/slot field operands.

Version 103 added the optional opaque `PortableMir { bytes }` payload. The envelope
content hash covers the payload, and a `PortableMir` section records its presence
and fingerprint. Its declared byte count is bounded before element decoding; the
complete artifact, including MIR, must fit the existing 64 MiB limit. Runtime ABI
v102 and helper ABI v6 were unchanged by that envelope-only revision.

Version 104 adds the uninhabited Never semantic and physical type and changes the
DebugPanic result contract to Never. Runtime ABI v103 and portable MIR format v2
reject previous products; helper ABI v6 is unchanged. No runtime Value can satisfy
a Never parameter or result, and verifiers reject normal returns from Never
functions even when a forged return register is also marked Never.

The bytecode crate does not depend on MIR and does not interpret this payload.
Bytecode-only loading validates envelope integrity, resource limits, metadata and
bytecode contracts. Native preparation must additionally decode the versioned
`KMIR` v8 input, check its runtime/helper ABI versions, reverify the complete MIR
program and rebuild program-point analyses. It then lowers MIR through the canonical
frontend-free bytecode path and compares the entire resulting program encoding.
Independent payload checksums are insufficient. Preparation precedes script effects.
Verification state is not serialized, and the decoded input does not retain source
text or source-analysis arenas.

SDK source emission includes portable MIR by default. `NativeInputExport::BytecodeOnly`
explicitly omits it; such artifacts remain interpreter-loadable and lack input for
native compilation. Presence alone does not prove native eligibility: malformed or
inconsistent MIR and backend-unsupported functions must be distinguished during
preparation. Compiler-generated payloads come from the same verified program as
bytecode, replacing any caller-supplied opaque payload in low-level build options.

Version 70 and runtime ABI v70 name the native iterator type and instruction Iter.
The former Cursor type name is removed from the source API. Iterator remains the
stepping protocol, and Iterable::Iter remains its independent associated type.
Earlier artifacts are rejected before execution; no compatibility alias or decoder
is provided. Runtime-helper ABI remains v6.

Version 62 and runtime ABI v62 add the ArrayJoin native contract used by string
interpolation and `[String].join`. Previous artifacts are rejected before execution;
the runtime-helper ABI remains v6 and KHI remains v11.

Version 61 and runtime ABI v61 add the verified MapResultError operation to
change an Err payload while preserving its origin. KHI remains v11; helper ABI
remains v6. Prior products are rejected without migration.

Version 60 and runtime ABI v60 preserve source URIs and one-based line/UTF-8 byte
columns for failure stacks, even without optional debug metadata. Runtime helper
ABI v6 publishes native instruction offsets before charging their budget.

Version 59 and runtime ABI v59 add iteration contracts, Iter ABI types and
verified native iterator instructions. Script iterators use ordinary linked calls.
Prior products are rejected; KHI remains v11.

Version 58 and runtime ABI v58 register explicit conversion contracts, derived
reverse conversion proofs and static associated conversion calls. Prior products
are rejected without migration; KHI remains v11.

Version 57 and runtime ABI v57 register read-only Index. Custom reads are ordinary
linked calls; builtin Array reads retain aggregate-index instructions. KHI remains v11.

Version 56 and runtime ABI v56 register Neg/Not with associated outputs. No
short-circuit or compound-assignment override is introduced.

Version 55 and runtime ABI v55 register generic arithmetic contracts with associated
outputs. Applied standard signatures are validated by the same portable rules as
source checking; matching numeric operations remain direct instructions.

Version 54 and runtime ABI v54 add Ordering and builtin comparison protocol calls.
Canonical trait contracts include their declared parameters and associated outputs.

Version 53 and runtime ABI v53 add guarded custom-key preparation and commit
intrinsics. Custom comparison/hash methods and generated composite helpers use
ordinary linked function slots. Verification accepts script Struct/enum protocol
overrides, checks defining-module ownership and explicit prerequisite impls, and
rejects host equality/hash overrides. KHI remains v11.

Version 52 and runtime ABI v52 add checked object identity equality and inequality
operations. Value categories without identity are rejected by source checking;
runtime identity operations validate object category and handle ownership.

Version 51 and runtime ABI v51 add standard protocol intrinsic calls and the
canonical standard declaration registry. Artifacts cannot redefine the reserved
`kagari-std` package. The verifier
checks standard impl contracts and applied bounds using the same canonical
protocols. KHI v11 rejects previous declaration products.

Version 50 and runtime ABI v50 add concrete typed standard-enum construction,
variant tests and payload reads, plus Option conversion intrinsic identities.
Verification checks operand/result representations, family arity and bounded
concrete ABI types before execution. `?` and script combinators lower to ordinary
branches, calls and returns; no exception-unwinding instruction is introduced.

Version 49 and runtime ABI v49 encode separate trait and associated-constructor
arguments in projection nodes, constructor-owned parameter identities and bounds,
and impl family bodies. Decoding bounds their arrays and type nodes; linked
verification rechecks arity, binder ownership, method contracts, input and output
bounds, recursion and static-only interface restrictions before execution.

Version 48 and runtime ABI v48 added checked scalar associated constants and
default values in canonical `const-v1` form. Constant-bearing traits and their
descendants cannot produce dynamic interface values.

Version 47 and runtime ABI v47 retain trait default-method ordinals and each
function's original source-module slot. Script default bodies are specialized
into impl-owned method instances with ordinary verified call slots. Source-module
slots must be local or reachable through the retained dependency closure; method
ordinals must be unique and refer to existing declarations. This adds no runtime
fallback lookup or runtime specialization.

Version 46 and runtime ABI v46 retained applied supertrait contracts and the explicit
`UpcastInterface` operation. Loading checks bounded acyclic inheritance, required
parent implementations, concrete parent dispatch tables and upcast ancestry
before execution. Parent tables are compiled ahead of time and retain the same
execution family as the original interface.

Version 45 and runtime ABI v45 added explicitly marked concrete host bridge tables
and KHI v10 associated-output declarations. Output identities, bounds, signatures
and the exact forwarding host call are verified before execution. Host bridge
tables do not introduce additional language implementations. Host declarations
include associated output types in their fingerprint and required type closure.
Standalone KHI versions before 11 are rejected without migration.

Version 44 added ordered concrete impl arguments to linked interface table
records. Generic tables are deduplicated by declaration and arguments; each
executable instance contains all its concrete method slots. Loading validates
argument arity, concreteness, bounds, uniqueness and exact method instance keys
against the retained template. A generic template record is not an executable
instance. Runtime ABI v44 uses these concrete slots; it does not specialize
methods at execution time.

Version 24 encodes each ABI type as bounded flat preorder nodes. The decoder
checks at most 4,096 nodes and depth 64 before rebuilding a recursive type, so
untrusted ABI types cannot grow the decoder call stack without a checked bound.
Module, function and instruction sequence lengths are checked as the decoder
reads each sequence header, before reading their elements. Their limits are
1,024, 65,536 and 1,000,000 respectively; aggregate post-decode limits still
apply across modules and functions. Module-owned table vectors have a
1,000,000-record sequence limit; concrete layout and public ABI member vectors
have a 4,096-record sequence limit. Function metadata, debug tables, artifact
section tables, verification summaries and signature lists use the same
1,000,000-record preflight limit as artifact tables.
Per-instruction operand vectors (calls, aggregate constructors and dynamic path
arguments) preflight at 4,096 registers; the executable program also limits
their combined count to 1,000,000 before verification or fingerprinting.
Construction measures the canonical encoded size of the program and build
metadata before verification, then measures the completed artifact before
fingerprinting. In-memory loading and encoding repeat the 64 MiB size check.
This also bounds strings and signature bytes without a second, conflicting
per-field byte limit. Byte decoding checks the input length before reading it.
In-memory artifact checks include function-table parameter layouts and the
parameter/local/register vectors in both attached and detached debug frame
layouts before validation, fingerprinting or encoding.
Embedded host interfaces and standalone KHI declarations preflight their type,
function and field-path lists at 1,000,000 records, and fields, methods,
parameters and path segments at 4,096 records. In-memory host declaration
validation enforces these limits before encoding or linking.
Every serialized module and declaration identity checks its path length before
reading segments, with a limit of 64. This applies to artifact headers and
embedded identities as well as standalone host declarations.
In-memory artifact construction, loader validation and encoding also reject
overlong module, host and public ABI identities before fingerprinting or
publication.

Version 23 stores complete concrete ABI types for struct fields. Runtime ABI v24
checks nested field values before allocation or replacement, including nominal
identity and container element types. Prior representation-only layouts are rejected.
Validation compares concrete struct instances with their public templates, including
instances emitted only by dependent modules. Equal physical representations do not
permit different field types or permissions. Public aggregate templates are validated
even without executable instances: declaration and member names must be nonempty,
aggregate names and member names must be unique in their respective scopes, and
structs cannot carry variants nor enums fields. The same check validates binders
and member types for IR and bytecode.

Version 20 adds the required host path contract fingerprint to each IR/bytecode
path record. Loading links each module-local path to exactly one registered runtime
descriptor with that contract, checks operand representations and write access,
and stores the resulting binding in the immutable executable version. Missing or
ambiguous bindings reject publication. Path IDs are never runtime descriptor IDs.
Reload path fingerprints cover the contract and operand shape; diagnostic labels
are excluded.

Version 21 carries portable field path declarations in required host interfaces.
KHI v6 uses the same records, including field identities, access, schema and
capabilities. Linking rejects required field paths without a unique matching
runtime binding before program publication.
Version 26 replaces those field-only records with `HostPathDeclaration`
records. Ordered field, index and virtual segments share one portable contract;
the loader rejects older artifact and interface formats before execution.
Version 27 adds the declaration identity of each interface implementation to its
public ABI record. Verification requires a unique local `Impl` identity, method
binders owned by it, and a complete method roster and substituted signatures for
local public traits. Reload
keys encode this identity canonically; the human-readable
`Type as Trait` label is diagnostic only. Older formats are rejected before execution.
Version 28 carries concrete function declaration identities and type arguments in
both executable functions and their directory records. The verifier checks their
agreement, module ownership, concrete type arguments and uniqueness; source
lowering always emits identities. Identity argument vectors obey the artifact
record limits. Older formats are rejected before execution.
Version 29 carries executable interface implementation tables. Each table uses
the public implementation declaration identity and maps trait method identities
to concrete function slots. Verification requires a matching public table,
checks method and implementation ownership, and requires exactly one executable
slot for each non-generic method of a concrete implementation. Source lowering
emits uncalled concrete implementation methods so those slots remain linkable.
Generic implementations and methods retain only their reachable instances;
runtime interface dispatch is a separate linking step. Table and method counts
obey the artifact resource limits. Older formats are rejected before execution.
For generic implementations, the table owns implementation binders and bounds;
each method record owns only its additional method binders and bounds. Executable
method slots carry the full concrete argument list in implementation-then-method
parameter order. Verification rejects slots with a different argument count.
Applied local generic trait arguments remain part of the public interface-table
type and method contract; they cannot be replaced by a bare trait declaration.
Version 30 records applied trait arguments in generic bound constraints, including
inline and `where` bounds. Canonical ordering compares the complete structural
trait type. Verification checks bound argument shapes and identities; artifact
resource limits include their argument vectors. Older formats are rejected.
Version 11 adds ordered enum payload types to public variant ABI records. These
use structural `AbiType` encoding, preserving nominal declaration identity and
container arguments instead of display strings or erased instruction ValueTypes.
Changing a payload type therefore changes the public ABI fingerprint and rejects
reload before publication. Body-only changes retain the same enum ABI.
Version 12 adds nominal enum/variant layout tables and `MakeEnum` layout/variant
slots. IR and bytecode verification check declaration ownership, payload type
references, slot existence, argument count and representations. Program linking
rejects conflicting layouts for the same nominal declaration across modules.
Public enum ABI records must match their executable variant names and payloads;
recomputing an artifact checksum cannot authorize a contradictory ABI record.
Version 13 records each nominal ABI type as a declaration identity plus ordered
type arguments. Conversion from checked types and fingerprinting preserve nested
arguments. Version 14 adds concrete type arguments to struct and enum layouts;
verification and program linking compare the complete instance identity. Public
type records retain their generic parameter declarations. Public enum payload
templates encode parameters by declaring owner and position, and must instantiate
to each executable layout, including instances emitted only by an importer.
Unused public templates still validate parameter ownership and position. Template
parameters are rejected in executable layout arguments and enum payloads.
Version 15 stores host value types as bounded flat preorder nodes and supports
nested tuple/container/standard-enum host contracts. Type encoding has a 4096-node
and 64-depth limit, including during artifact decoding. Runtime call boundaries
validate nested host arguments and results instead of accepting any heap object.
Version 16 includes portable host type/member declarations in each required host
interface. Validation checks member ownership and referenced-type closure. Host
interface fingerprints include those contracts with documentation removed; host
section counts include both function and type declarations.
Version 17 extends structural `AbiType` encoding to public parameters, results,
const types, struct fields, trait methods and interface implementation targets.
Generic binders use declaration owner and position rather than parameter spelling.
Constraints encode standard constraint tags or nominal trait identities, grouped
by parameter identity and sorted/deduplicated. Inline and `where` forms produce the
same bound contract. Trait receiver templates carry their owning trait identity;
`Self` cannot escape into a concrete signature or executable layout. Shared IR and
bytecode validation rejects unbound/foreign template parameters, noncanonical
bounds, invalid nominal kinds and wrong standard-enum argument counts. Public
top-level functions remain concrete. Names retained on members are declaration
labels; display strings no longer encode the types of public members.
Version 18 adds `AbiType::Host(DefinitionId)` and the distinct `HostHandle`
operand representation. Source lowering carries required portable host types,
including the transitive closure of member references. IR and bytecode validation
reject semantic host references absent from the required interface, including
annotation-only public signatures and concrete layout arguments. Registration and
linking still validate full declaration contracts before execution.
Version 19 permits required host call records with member declaration identities.
These records must equal the declaring host type's generated method contract,
including its explicit nominal receiver and passing style. KHI v8 carries the
same checked method call contracts; old interface versions are rejected.
Version 31 and runtime ABI v31 add the KHI v8 host trait implementation table to
portable host type contracts. Old KBC and KHI products reject before execution.
Version 32 and runtime ABI v32 add ordered applied trait arguments in KHI v9;
distinct applications on one host type are separate identity keys. Older KBC and
KHI formats are rejected before execution.
Version 33 and runtime ABI v33 add bounded private trait contracts to executable
modules. Public traits retain their single public ABI record; private contracts
allow loading to recheck host method mappings even when the trait is not
exported. Version 32 products are rejected without migration.
Version 38 and runtime ABI v38 require program-point and lexical-scope local
visibility in debugger metadata. Earlier products, which could expose locals
outside their scope, are rejected before execution.
At the v38 checkpoint, the runtime ABI identity was `kagari-runtime-abi-v38` and
runtime-helper ABI was v5. Previous ABI artifacts are rejected even when requested
by the caller: v5 lacks shared mutation accounting; v6 lacks prepared path commits and quarantine;
v7 lacks root-call sessions and cancellation; v8 lacks scoped host contexts and
checked synchronous script reentry; v9 lacks session-owned frame stacks and
nested execution observation; v10 lacks session-registered host resources, owned
borrow tokens and contextual path callbacks; v11 lacks enum version handles and
typed standard-enum tags. Runtime ABI v16 additionally requires nominal host type
bindings and registry-owned host roots. ABI v17 requires complete member contract
linking and declaration-derived registration. ABI v18 requires structured public
type and bound contracts for reload validation. ABI v19 requires the distinct host
handle representation and source nominal host contracts. ABI v20 requires
declaration-derived method binding and receiver contracts. ABI v21 requires
contract-linked path slots. ABI v22 requires declared field path bindings; all prior ABI products
are rejected.
ABI v26 requires complete portable path declarations, including index and virtual
segments, in the required host interface.
ABI v27 requires identity-bearing interface implementation records and rejects
display-label-only reload keys.
ABI v28 requires identity-bearing executable function records.
ABI v29 requires verified executable interface method tables.
ABI v30 requires applied trait bound identity in public signatures and templates.
The helper ABI preserves cancellation
and commit EngineFault independently of resource/trap status.
Host calls use HostImportId operands and a required HostInterface declaration
table. There is no arbitrary host-registry
fingerprint option or duplicate string dependency table. The obsolete BuiltinMethod
call operand is also absent;
standard-library calls use StandardIntrinsic and its verified call contract.
The current format also excludes artifacts produced by the old generic
template lowering: current compilation emits concrete function instances and
does not treat uninstantiated generic parameters as heap-object representations.
`from_bytes()` checks magic and the current version before decoding, imposes a
64 MiB encoded-size and decoding budget, and rejects trailing data. Decoding alone
does not establish trust: header, content, dependency, and bytecode checks still
run before execution.

`KbcArtifact::from_program()` and `VerificationMetadata::from_program()` return
structured validation errors for invalid programs. Both verify before deriving
metadata or computing fingerprints. In-memory loader validation likewise checks
bytecode before hashing, so invalid host type declarations cannot cause a
serialization panic. A checksum cannot substitute for bytecode validation.

The language contract version is `kagari-language-v2`, independent of Rust crate
versions and the binary format version. Artifacts carrying the former crate-based
language version are rejected before execution; they may encode older assignment
evaluation rules even when their binary layout is readable.

Source analysis, IR and bytecode carry the same `ModuleIdentity { package, path }`.
Artifact header and loader copies must equal the identity carried by the bytecode.
Physical source names are diagnostic metadata, separate from logical identity.
Artifact build options cannot override identity after analysis.

Compatibility fingerprints use FNV-1a-64 over this canonical serialization with
the `kagari-canonical-v2` domain prefix. Rust `Debug` output is never fingerprint
input. The artifact content fingerprint includes the header (with its content
fingerprint zeroed) and every payload section. This is a compatibility checksum,
not cryptographic authentication; signature policy is a separate host concern.

Public scalar const values use the `const-v1:` encoding followed by the type and
value: bool is `0` or `1`, i32 is decimal, f32 is eight lowercase hexadecimal
digits of IEEE bits, and String is its decimal UTF-8 byte length, a colon, and
the UTF-8 contents. Unit has tag `unit` and no payload. This preserves signed
zero and string boundaries without relying on Rust formatting traits for values.
Additional integer types use `const-v2:<type>:<decimal-value>`, including the
full u64/usize range. f64 uses `const-v2:f64:<bits>` with sixteen lowercase
hexadecimal IEEE-bit digits. The enclosing format/ABI version rejects old
artifacts before these values participate in linking.

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
- required host declarations in BytecodeModule.host_interface
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

- register and local layouts
- instruction effect metadata
- control-flow target metadata
- safepoint metadata
- GC root metadata
- typed path descriptor fingerprints
- public ABI fingerprints
- dependency fingerprints
- required host-interface fingerprints
- security profile requirements

The loader verifies every member even when metadata is present. Root-level function,
effect, control-flow, public-ABI and path summaries refer to the root ModuleRef;
dependency metadata remains owned by each BytecodeModule. The loader derives and
compares these summaries against the actual program. They cannot replace verification.
Root-map completeness and per-table/depth decoding quotas remain pending.

Dependency fingerprints carry ModuleIdentity and are derived from the canonical
bytes of every non-root module in program order. ArtifactBuildOptions has
no dependency-fingerprint input. ArtifactCompatibility may pin an exact dependency
set with Some; None omits that external pin but still requires metadata/payload
agreement. The required-host fingerprint covers each program member's declarations,
excluding documentation and declaration order within each interface. Runtime code
fingerprints derive from BytecodeProgram for both source and artifact loads; the
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
- local and captured value live ranges
- frame layout metadata
- type names
- path names
- line tables

Debug metadata may be stripped from production artifacts.
Stripping debug metadata must not change execution semantics.

## Signatures and Trust

Embeddings may require artifact signatures or hashes.

Signature policy is host-controlled.
The language runtime only requires that signature metadata, when present, be validated before loading the artifact as trusted cache content.

Unsigned artifacts may still be loaded in development profiles if host policy allows it.

## Compatibility Rules

An artifact is compatible only when all required versions and fingerprints match:

- artifact format version
- language version
- compiler compatibility version
- runtime ABI version
- runtime helper ABI version
- required host-interface fingerprint
- dependency module fingerprints
- public ABI fingerprints
- typed path descriptor fingerprints
- security profile requirements

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
  -> validate dependencies and required host-interface fingerprints
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

The required host-interface fingerprint is derived from the identity-sorted
declaration contracts, excluding documentation. Import slot order is irrelevant
to this ABI fingerprint; the content checksum still covers the exact module.
Loading checks the derived fingerprint and resolves every required declaration
against the actual runtime registry by nominal identity. Signature, borrow,
effect, permission and cost differences reject the module before publication or
initialization. Unrelated installed host functions do not affect compatibility.

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
including the full usize range. Numeric literals preserve their checked scalar
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
replacement intrinsics. Repeat counts have the semantic type usize; writable target
access and element types are checked before execution. Older products are rejected.

Version 82/runtime ABI v82 add immutable range ABI descriptors and canonical
encoding, MakeRange/RangeBound instructions, the standard Bound enum, lazy range
iteration and the normalized ArrayCopyWithinBounds intrinsic. Public copy_within
calls must lower their RangeBounds protocol calls before bytecode verification.
Validators reject missing endpoints, invalid element types, mismatched Bound
payloads and read-only destinations before execution. Earlier products are rejected.

Version 83/runtime ABI v83 restrict repeat-array instructions to element types
without shared mutable identities. Source analysis and portable verification share
the structural type predicate. ArrayList::from_fn lowers to ordinary control
flow and closure calls; an unlowered factory intrinsic is rejected. Older products
are rejected rather than interpreted with changed repetition rules.

Version 84/runtime ABI v84 replace native public access types with source-declared
List/MutableList, Map/MutableMap and Set/MutableSet interfaces. ArrayList,
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
intrinsics. Constructors validate the argument tuple (including usize limits)
and produce Iter<String>. Unlowered source bindings and previous artifacts are
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
no script calls, checks structure guards and budgets, and preserves identity while
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
window/chunk step reads source slots, it validates retained native source revisions
and reestablishes source guards released by early pipeline closure. A structurally
changed source traps; unchanged sources can continue from the saved position.

KBC/runtime ABI v101 adds the standard `Fn` protocol identity and callable
specializations. Callable adapters use the existing closure instruction and
semantic signature validation, including capture and return contracts. Earlier
artifacts are rejected; no legacy callable dispatch or migration is provided.
