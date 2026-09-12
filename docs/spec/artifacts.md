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
  module: BytecodeModule,
  tables: ArtifactTables,
  verification: VerificationMetadata,
  debug: DebugMetadata?,
  signatures: ArtifactSignatures?
}
```

Format version 6 uses `bincode` with fixed-width integers, little-endian byte order,
and declaration-order fields. Any change to this representation requires a new
format version. Versions 1 through 5 are rejected; no migration or compatibility
decoder exists. Version 6 removes the obsolete BuiltinMethod call operand;
standard-library calls use StandardIntrinsic and its verified call contract.
The current format also excludes artifacts produced by the old generic
template lowering: current compilation emits concrete function instances and
does not treat uninstantiated generic parameters as heap-object representations.
`from_bytes()` checks magic and the current version before decoding, imposes a
64 MiB encoded-size and decoding budget, and rejects trailing data. Decoding alone
does not establish trust: header, content, dependency, and bytecode checks still
run before execution.

The language contract version is `kagari-language-v1`, independent of Rust crate
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
- host dependency table
- string table
- source file table
- debug name table

Hot execution paths use table ids, not repeated strings.
Strings remain available for diagnostics, debug metadata, and tooling.

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
- host registry fingerprints
- security profile requirements

The loader may re-run verification even when metadata is present.
Metadata is a validation aid, not a replacement for verification.

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
- host registry fingerprint
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
  -> validate dependencies and host registry fingerprints
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
