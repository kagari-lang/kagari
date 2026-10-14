# Kagari Module Loading Specification

This document defines how source files, module ids, imports, package roots, and bytecode artifacts are resolved.
Module execution semantics are defined in [modules.md](modules.md).

## Design Goals

- make module identity stable across source loading, bytecode artifacts, hot reload, and diagnostics
- keep import resolution deterministic
- support single-file scripts and package-style projects
- allow `.kbc` artifacts to replace source parsing when valid
- avoid hidden mutable module storage
- keep host embedding policy in control of file-system access

## Module Identity

Each loaded module has:

```text
ModuleIdentity {
  package_id: PackageId,
  module_path: ModulePath,
  source_uri: SourceUri,
  module_id: ModuleId
}
```

`module_id` is the stable runtime identity used by module epochs, reload, bytecode artifacts, JIT artifacts, and diagnostics.

The identity must not depend on temporary absolute paths when the module belongs to a package root.
For embedded single-file scripts, the host supplies the package id and source uri.

## Package Root

A package root is a host-approved directory or virtual source collection.

Package roots define:

- package id
- root source directory
- allowed source file extension
- optional artifact cache directory
- import search policy
- host installation defaults
- host-provided dependency mappings

The package manager name `kg` is reserved for future tooling, but the language runtime only requires the package-root abstraction.

## Source Files

Kagari source files use the `.kgr` extension.

The loader may accept virtual source files supplied by a host application.
Virtual source files still receive stable source uris for diagnostics, caching, and reload tracking.

## Import Resolution

Imports resolve through module paths, not arbitrary string paths.

Resolution order:

1. current package modules
2. explicitly configured dependency packages
3. host-provided virtual modules
4. builtin modules

The host may restrict which resolution roots are available.
Failed imports produce diagnostics during loading or checking.

## Source-to-Module Mapping

Within a package root:

```text
foo/bar.kgr -> foo::bar
foo/mod.kgr -> foo
main.kgr -> package entry module, if selected by host or CLI
```

The exact physical layout can evolve, but a package must not contain two source files that resolve to the same module path.

## Builtin Modules

Builtin modules are provided by the runtime or host.
They are resolved by stable module identity, not by files on disk.

Builtin modules include standard language facilities defined in [builtins.md](builtins.md).

## Bytecode Artifact Loading

The loader may load a `.kbc` artifact instead of source when:

- the artifact module id matches the requested module
- the artifact format version is supported
- dependency fingerprints match
- host registry fingerprints match
- required runtime bindings are installed
- debug metadata policy is satisfied

Invalid or stale artifacts are rejected or ignored according to host policy.
They must not be executed without verification.

## Module Cache

The loader may cache:

- parsed syntax trees
- checked modules
- bytecode artifacts
- dependency fingerprints
- diagnostics

Cache entries are invalidated by source hash, dependency fingerprint, host registry fingerprint, compiler version, language version or installed interface changes.

## CLI Loading

The CLI uses the same loader model as embedded hosts.

The current executable is `kagari-cli`; the separately planned `kg` distribution
rename has not been applied. Commands select source or project work explicitly:

- `parse <file.kgr>` parses one file.
- `check [file.kgr]` checks a file, or the selected package closures when omitted.
- `build` emits a complete artifact for each selected package without execution.
- `emit [-o file.kbc] <file.kgr>` emits a standalone file's artifact.
- `run [file.kgr]` compiles and invokes the selected root's separate `main` entry.
- `run-artifact <file.kbc>` validates and executes an existing artifact without
  reading package manifests or source files.

An implicit source path runs source and an implicit `.kbc` path runs an artifact.
`--jit` and `--no-jit` select execution policy; they are rejected for other commands.
Project options cannot be combined with an explicit source or artifact path.

For project commands, find the nearest `Kagari.toml` from the invoking directory,
or use `--manifest-path <Kagari.toml>`. The package loader discovers the owning
workspace and applies member/default-member rules. `-p` / `--package NAME` may be
repeated; `--workspace` selects all members and is mutually exclusive with `-p`.
`run` requires exactly one selected package. Building a library requires no `main`.

The shared root `Kagari.lock` covers all workspace members even when only one is
checked/built. `--locked` prohibits graph changes and requires a lock; `--offline`
prohibits network acquisition; `--frozen` combines both. Current acquisition is
local only. Source-only edits may be rebuilt under an unchanged locked graph.
The CLI supplies no implicit immutable host packages or remote package registry.

The CLI admits declared local inputs under the invoking filesystem anchor (a
Windows drive/share root, or `/`); `--input-root DIRECTORY` narrows that read root.
Embedding hosts supply their own approved providers. Only declared module trees
and dependencies are captured, with the loader's canonical-path and resource limits.
Workspace ownership, dependency names and source visibility remain package/HIR rules.

All project members share `<workspace>/target/kagari`: generated native declaration
views live under `declarations`, and complete `.kbc` products under `programs`.
Products are addressed by checked source/selection/alias inputs, native and offline
Host contracts, query limits, the compiler contract identifier and emitted bytes.
Matching complete inputs and output bytes can share a file; edits retain earlier
products. Publication of shared
products is atomic and checks existing bytes before reuse. This is output storage,
not a cache that skips source checking or compilation. `build -o PATH` selects an
explicit output and therefore requires exactly one selected package.

Module reload remains a host-managed embedding operation. CLI convenience behavior
must not define different language semantics from embedding behavior.

## Security

Module loading observes security policy.

The host controls:

- readable source roots
- writable artifact cache roots
- available dependency packages
- whether bytecode artifacts may be loaded
- whether debug metadata may be loaded
- whether dynamic module loading is allowed at runtime

Scripts do not receive unrestricted file-system access through import syntax.

## Hot Reload

Reload uses the same module identity as initial load.

The reload candidate may come from:

- changed source
- changed `.kbc` artifact
- host-supplied virtual source
- generated module content

Reload validation is performed by the runtime before publication.
The loader supplies source and artifact identity data needed for diagnostics and dependency checks.

## Acceptance Criteria

Module loading is complete when:

- module ids are stable across source, artifacts, reload, and diagnostics
- imports resolve deterministically through package roots and host-provided dependencies
- duplicate module paths are rejected
- stale `.kbc` artifacts cannot execute
- loader cache invalidation respects source, dependency, compiler, runtime, and host registry fingerprints
- CLI loading and embedding loading share the same semantics
