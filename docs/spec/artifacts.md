# Kagari Bytecode Artifact Specification

This document defines the `.kbc` artifact boundary.
The semantic bytecode model is defined in [bytecode.md](/Users/mikai/CLionProjects/kagari/docs/spec/bytecode.md).

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

The exact binary encoding is an implementation detail as long as the logical sections remain versioned and validated.

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
- function names
- local names
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
