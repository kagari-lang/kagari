# Package and Dependency Design

Status: proposal for later implementation. Kagari needs a package boundary for
source organization, imports, dependencies and reproducible builds, without making
package distribution or filesystem access part of VM execution. This design uses
Cargo-style dependency declarations and separates package resolution from runtime
publication. The companion [update model](update-model-design.md) owns compatible
hot reload and state replacement.

The user approved following Rust's dependency model as a direction. Manifest names,
the initial feature subset and version restrictions below are recommendations to
review, not implemented features or final language specifications. No implementation
or commit is authorized by this planning task.

## Current foundation

- [PackageId and ModuleIdentity](../crates/kagari-common/src/identity.rs) already
  separate a string package identity from a module path. They do not constitute a
  package manifest, version resolver or lockfile.
- [Module semantics](spec/modules.md) support registered source modules, imports,
  visibility and dependency closures. Analysis does not discover unregistered files.
  Module cycles are allowed; `pub(crate)` is not currently supported.
- [Loading specifications](spec/module-loading.md) describe host-approved package
  roots and dependency mappings, and reserve `kg` for future tooling. They are not
  evidence of an implemented general package manager.
- General packages use common identities and source handoff; native declaration
  installation is a separate boundary, not an implicit package privilege.

Re-audit these foundations at activation. Existing specifications govern until
an implementation updates them.
Preserve [source-module execution tests](../crates/kagari-embed/tests/source_modules/execution.rs)
and [source snapshots](../crates/kagari-embed/tests/source_snapshots.rs) when changing
resolution and input registration; they are not full package-manager acceptance.

## Responsibilities

| Concept | Meaning |
| --- | --- |
| Package | Named source/interface unit with a root module and declared dependencies |
| Module | Namespace within one package |
| Resolved package graph | Exact package selections and dependency-alias edges for one build |
| Program | Prepared executable closure selected from that graph |
| Script | Runtime-local logical installation of a Program under the host API proposal |

A package is not a runtime heap and is not automatically an atomic update unit.
One Program can contain code from multiple packages; multiple Script installations
can independently use the same package. Building a library does not run an entry
function. Running a tool or script remains an explicit host/CLI function call.

The first version should have one root code target per package, usable as a library
or through exported entry functions. Do not reproduce Cargo's entire target matrix,
build scripts, procedural macros, feature unification or platform dependency system
before Kagari needs them. Unsupported manifest fields must be diagnosed, not ignored.

## Manifest and dependency declarations

Proposed filenames are `Kagari.toml` and `Kagari.lock`. The shape borrows Cargo's
package metadata and dependency tables; exact compatibility with Cargo files is not
promised. Cargo supports registry/version, path and Git dependencies, including
dependency renaming. These inform the declaration model, not VM loading behavior.
See [Cargo dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)
and [manifest structure](https://doc.rust-lang.org/cargo/reference/manifest.html).

```toml
[package]
name = "gameplay"
version = "1.0.0"
root = "src/lib.kgr"

[dependencies]
combat = { path = "../combat" }
rules = { package = "shared-rules", path = "../shared-rules" }
```

`rules` is the local import alias; `shared-rules` is the actual package name.
Resolve relative paths against the declaring manifest, not the process working
directory. Package roots must pass host path/canonicalization policy, including
symlinks and duplicate physical sources. A permitted `../combat` dependency is not
arbitrary script filesystem access.

The target declaration vocabulary can also include:

```toml
[dependencies]
combat = "1.2"
rules = { package = "shared-rules", version = "2.0" }
```

Git sources would use `git` plus an optional `rev`, `tag` or `branch`, resolved to
an exact revision before building. For implemented source kinds, follow documented
Cargo-style version requirement semantics rather than inventing a similar-looking
but different range language. Unsupported source kinds must fail clearly.

Recommended first delivery: path dependencies and host-supplied resolved packages,
with a lock/graph contract capable of carrying future Git/registry selections.
Remote fetching, registry publication and a full multi-version solver can follow.
This staging is a review choice, not a claim that Cargo only supports these features.

## Resolution and locking

Separate requirements from selections: the manifest describes requirements while
the resolved graph/lock records exact choices. This follows the distinction in
[Cargo's manifest and lockfile guide](https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html).

Resolution belongs to host tooling before compilation. Runtime import/call/reload
must not perform network requests, update a lock or select a newer dependency.
Offline/locked operations fail if the exact requested inputs are unavailable or
inconsistent; they never silently substitute another package.

Lock records should identify canonical source, package name, selected version,
exact remote revision/checksum where applicable and resolved dependency edges.
Mutable path/virtual sources additionally produce a captured build snapshot and
content digest. A lockfile alone does not freeze the contents of a local checkout.
Deployment publishes the exact resolved graph and build fingerprints, not just a
manifest with version ranges. Hashes establish integrity/cache identity, not trust
or authorization to install Native implementations.

Proposed first-version restriction: select one revision of each canonical package
family per Program graph and reject conflicting requirements with a dependency
chain diagnostic. Supporting simultaneous versions is a later resolver/type-identity
extension. This restriction does not forbid old and new runtime generations from
coexisting during compatible reload. Independent Programs may resolve differently.

Package dependency cycles should be rejected in the initial build graph. Existing
legal cycles between modules within a package remain legal. Do not confuse a
namespace/reference cycle with a package acquisition/build cycle.

## Identity and revision

Keep these concepts distinct:

- Logical package family: canonical source namespace plus package name, assigned
  by trusted package resolution or the host for virtual packages.
- Package selection: declared version and exact source revision selected for a
  build. Dependency aliases point to selections; aliases are not type identities.
- Module/declaration identity: logical package family plus module/declaration path.
  Existing declaration identity must not change merely because unrelated items
  were inserted, source lines shifted or local import aliases changed.
- Build fingerprint: exact sources, resolved edges, host contracts and compiler/ABI
  inputs needed to identify a prepared executable product.
- Runtime generation: one published execution version in a particular installation.

These are conceptual roles, not a mandate for five new public ID structs. Finalize
their portable representation before changing artifacts. Package versions are not
runtime generations, and same-name packages from different sources are not the same
nominal type. Absolute checkout locations must not define portable package identity.

Under the proposed single-selection rule, logical family identity plus contract
validation can relate declarations across compatible revisions. The loader must
not mix two selections just because their logical declaration IDs match: exact
build/generation ownership and compatibility checks remain mandatory. Replacing a
package source/family is a different dependency, not an automatic compatible rename.
Future simultaneous-version support must add disambiguation before it is enabled.

## Modules and visibility

Proposed root is `src/lib.kgr`, with explicit manifest override. Existing module
declarations select child sources: `foo.kgr` or `foo/mod.kgr` can represent the same
child path but both together are an error. The package loader discovers/registers
approved sources into an immutable input snapshot; HIR still does not perform IO.
Virtual packages provide an equivalent root and module map without a real directory.

Inside a package, `crate::` means its root and `self`/`super` retain their existing
meaning. A dependency alias such as `rules::` resolves through that package's direct
dependency map. Transitive dependencies are not implicitly imported; a package may
re-export accessible items through its public API. A missing direct dependency is
diagnosed rather than found by searching every installed package by display name.

Reject conflicting dependency aliases, reserved roots and ambiguous bindings with
clear diagnostics. Existing local/import precedence must remain explicit; package
loading must not introduce a hidden fallback search order. Validate every module
and item visibility boundary through the existing resolver.

Keep `pub`, private and `pub(super)` semantics. Adding `pub(crate)` as package-wide
visibility is a useful follow-up recommendation but needs explicit language work;
do not document it as currently available. Root public re-exports can define a
convenient facade without merging distinct declaration identities.

## Script, standard and Native packages

Script source, bundled standard source and host declaration packages should expose
the same logical namespace/dependency contract to analysis. Runtime providers remain
explicit trusted implementations linked by the [Native contract model](spec/standard-declarations.md).
A downloaded package declaring a Native symbol does not register arbitrary Rust code.

The host supplies actual Native bindings separately from portable declarations.
Package resolution checks interface requirements; runtime linking checks installed
implementations. Standard-provider trust cannot be obtained by copying a URI or
manifest label. Initial standard package selection is pinned to the compatible
toolchain/provider contract; it is not an unversioned ambient dependency.

Host module registration from the [interop plan](rust-interop-design.md) occurs
inside a host package namespace. Short type names remain sufficient; package and
module ownership determine the complete declaration identity.

## Compilation and update handoff

The host API may accept a package input in addition to single-file sources:

```rust
// Target API sketch, not a current method.
let program = engine.compile(Source::package("Kagari.toml"))?;
let script = runtime.load(&program)?;
```

Single-file embedding synthesizes a host-owned package identity and does not require
a manifest on disk. The default facade should not expose every resolver record.
An advanced resolved-source-graph input permits virtual sources and offline builds.

Compilation selects the root and complete reachable source closure, validates the
resolved graph and produces a Program with exact package/module provenance. Package
artifacts or caches can be implementation details; the first release need not
promise independently linkable per-package binaries. Cache keys include dependency
and host-contract fingerprints, not just source paths or package version strings.

Edits may occur anywhere in a loaded dependency graph. Determine all affected
installed roots through reverse dependencies. First implementation may rebuild
each affected Program conservatively; later incremental compilation can reuse
unchanged modules. Generic instantiations, inlining and embedded constants require
caller invalidation even when callable signatures have not changed.

The [update model](update-model-design.md) decides whether a candidate is compatible
or needs state replacement. Changing one package does not imply overwriting one
live code table. Each installation publishes a coherent Program generation;
updating multiple installations/Actors is explicitly host-coordinated. Unupdated
installations retain their existing pinned selection, not a globally mutated package.

## Review choices and implementation sequence

Before implementation, confirm the following recommendations:

1. `Kagari.toml`/`Kagari.lock`, one root target and `src/lib.kgr` by default.
2. Path and virtual/resolved host packages first; remote distribution later.
3. One selected revision per package family per Program initially.
4. Acyclic package dependencies, while preserving intra-package module cycles.
5. Whether package-private `pub(crate)` belongs in the initial language scope.

These choices do not change the approved Cargo-inspired dependency direction.
No new implementation track begins now. The [roadmap](implementation-roadmap.md)
must sequence the package work with EP/RI/HA instead of creating circular phase
dependencies. Freeze identity and resolver contracts before finalizing HA's package
input and update compatibility; package code need not depend on an implemented SDK
builder spelling or on async.

- [ ] PK00: Re-audit ST/NR results; confirm review choices, package identity and
  source/lock formats. Inventory current module/import tests and artifact impact.
- [ ] PK01: Implement manifest/virtual package normalization, deterministic module
  maps and dependency/alias validation. Test diagnostics and snapshot isolation.
- [ ] PK02: Implement supported resolution/locking and content snapshots with
  host-approved sources, offline behavior and the selected version-conflict policy.
- [ ] PK03: Integrate resolved graphs with HIR, Program/artifact provenance, SDK/CLI
  inputs and reverse-dependency invalidation. Keep IO out of compiler execution.
- [ ] PK04: Update specifications/examples and run package, feature and compatibility
  acceptance. Remote providers are separate scope unless explicitly activated.

## Acceptance and verification

Cover same-named packages from different sources, dependency aliases, missing direct
dependencies, diamond graphs/conflicts, cycles, duplicate module paths, virtual/path
equivalence, checkout relocation and standard/host namespace spoofing. Test lock
reproducibility, local source changes, unavailable locked input, path traversal and
symlink policy, cancellation, bounded graph size and atomic publication of inputs.

Run end-to-end single-file and multi-package source/artifact routes, including
changes to a dependency function, generic body and constant. Verify affected roots
are identified, unchanged code can be reused without stale behavior, and no package
fetch or initialization occurs at runtime. Keep the existing source-free/backend
feature matrix and all repository final checks:

```text
uv run --locked scripts/check_structure.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Buildable checkpoints are the default; record any explicitly approved intermediate
failure with command, cause and owner. Update formats and consumers directly without
compatibility-only readers or duplicate resolution semantics. No performance result
is implied by this proposal.
