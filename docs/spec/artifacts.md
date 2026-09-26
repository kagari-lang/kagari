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

## Logical Layout

An artifact contains:

```text
KbcArtifact {
  header: ArtifactHeader,
  program: BytecodeProgram,
  tables: ArtifactTables,
  verification: VerificationMetadata,
  debug: DebugMetadata?,
  signatures: ArtifactSignatures?
}
```

Format version 43 uses `bincode` with fixed-width integers, little-endian byte order,
and declaration-order fields. Runtime path binding identity uses index and
virtual segment fingerprints from resolved contract fields. Versions 1 through 41 are rejected; no
migration or compatibility decoder exists. The format stores a complete
stable ordered BytecodeProgram, its
root ModuleRef, and module/function call slots. Structs use nominal layout tables,
positional initializers and layout/slot field operands.

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
The current runtime ABI identity is `kagari-runtime-abi-v38`; the runtime-helper ABI is
v5. Previous ABI artifacts are rejected even when requested by the caller: v5
lacks shared mutation accounting; v6 lacks prepared path commits and quarantine;
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

Format 43 (language v3, runtime ABI v43) records trait-owned associated member
identities, declaration bounds, impl output definitions, interface equality
bindings and projection templates. Generic bound targets are structural ABI
types, so projections have the same canonical identity as parameter bounds.
Associated binding maps are ordered by declaration identity; decoding rejects
duplicate or noncanonical keys and applies the existing bounded type node,
depth, identity and record budgets. Linking verifies the complete output schema
and bounds even when no trait method is executed. Earlier formats are rejected.
