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

## Module Identity and Package Roots

`ModuleIdentity { package: PackageId, path: Vec<String> }` is the portable logical
identity. A package root has `path: []`; `mod combat` extends it to `["combat"]`.
Physical source origins and file revisions are separate diagnostic inputs. Runtime
handles and epochs identify an installation/generation, not another source field
inside `ModuleIdentity`. [Package identity](packages.md#portable-identity) defines
relocation-stable families and exact selections.

An approved filesystem directory or virtual source tree supplies package inputs.
The root manifest supplies a namespace, or the host supplies one; if both do, they
must agree. The manifest and caller's dependency catalog define available source
packages. Installed native APIs are explicit engine inputs. A root cannot grant
native registration privileges through its name or source contents.

Single-file embedding needs no manifest and synthesizes a distinct unnamed root
whose package identity derives from the normalized source name. Hosts requiring
identity across file moves can supply explicit module bindings. General package
identity comes from the portable namespace/locator, not an absolute checkout path.

## Source Discovery and Imports

Sources use `.kgr`. A package's manifest selects one root, defaulting to
`src/lib.kgr`. Discovery parses `mod` declarations before HIR analysis; it does
not recursively compile every file in a directory.

For `mod foo;`, exactly one of `foo.kgr` and `foo/mod.kgr` must exist relative to
the declaring module's directory. Both is an ambiguity; neither is a missing
module. Children of `foo.kgr` are found beneath `foo/`, as are children of
`foo/mod.kgr`. `mod foo { mod bar; }` searches `foo/bar.kgr` or `foo/bar/mod.kgr`.
Inline modules share their containing physical source and retain their own logical
path. Duplicate module identities or duplicate canonical physical sources are
rejected. Undeclared files are ignored. Virtual inputs follow the same rules.

Manifest, graph, traversal, source-byte and module-count limits bound acquisition.
Filesystem inputs enforce canonical containment in the approved root, including
symlinks. Discovery/capture errors and cancellation discard the candidate; source
handoff publishes the complete immutable closure atomically. HIR does no IO.

Imports use logical paths, not arbitrary string filesystem paths. `crate`, `self`
and `super` select source namespaces; a direct dependency alias selects the exact
root bound by that importing package. Transitive packages are accessible only
through declared direct dependencies or accessible re-exports. Workspace siblings
are not an ambient import search path. Installed provider aliases/preludes remain
explicit analysis inputs. [Module name precedence and visibility](modules.md) own
resolution; there is no separate loader fallback order.

## Builtin Modules

Builtin modules are provided by the runtime or host.
They are resolved by stable module identity, not by files on disk.

Builtin modules include standard language facilities defined in [builtins.md](builtins.md).

## Bytecode Artifact Loading

A complete `.kbc` Program can be loaded without source or package configuration.
Bounded decoding and executable verification validate its identities, formats,
root/member graph, public exports, layouts and call contracts before publication.
Required native/Host bindings must match installed contracts. Runtime ownership,
debug policy and reload compatibility remain independent checks; fingerprints do
not bypass them. [Artifacts](artifacts.md) owns the exact envelope/linking contract.

A host selecting a stored build product must also match its expected inputs.
Source-free execution does not fetch current package versions or compare against
an ambient checkout. Invalid artifacts never execute; choosing to rebuild instead
belongs to host/tool policy.

## Module Cache

Analysis may reuse immutable syntax and checked query products under their source,
selection and provider validity contracts. [Package build inputs](packages.md#build-inputs-and-updates)
define complete closure/environment identity and affected-root reporting. Current
CLI shared storage addresses emitted products but does not skip source checking or
compilation. Future persisted compilation caches must also account for actual
compiler revisions/options and continue executable validation.

## CLI Loading

The CLI uses the same loader model as embedded hosts.

The `kagari-cli` package provides the `kg` executable. Commands select source or
project work explicitly:

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
- which explicit host load/reload operations are available

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

- logical module identities survive checkout relocation and agree across source and artifacts
- imports resolve deterministically through package roots and host-provided dependencies
- duplicate module paths are rejected
- invalid artifacts and mismatched installed contracts are rejected before execution
- loader cache invalidation respects source, dependency, compiler, runtime, and host registry fingerprints
- CLI loading and embedding loading share the same semantics
