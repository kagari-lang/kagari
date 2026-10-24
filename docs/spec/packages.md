# Source packages and workspaces

Kagari distributes packages as source and compiles a selected package together with
its declared dependencies into one Program. Each package has one unnamed root
module and may expose many host-callable functions. There are no separate library
or executable targets, independently compiled package libraries or binary linker.
Loading a Program does not execute a function or initialize module state.

`kagari-package` owns acquisition and graph resolution. HIR checks an immutable
source environment; executable loading and calls do not read manifests, fetch
packages or update locks. [Module loading](module-loading.md) owns source discovery,
CLI commands and artifact handling; [modules](modules.md) owns names and visibility.
See the [runnable workspace](../../examples/package-workspace/README.md) and
[SDK host](../../crates/kagari/examples/package_workspace.rs).

## Manifests

A package uses `Kagari.toml`. Unknown fields and unsupported source kinds are errors.
The default root is `src/lib.kgr`; `root` can select another `.kgr` file.

```toml
[package]
name = "gameplay"
namespace = "example/gameplay"
version = "1.0.0"
root = "src/lib.kgr"

[dependencies]
rules = { package = "combat-rules", path = "../combat-rules", version = "1" }
service = { package = "service-api", host = "approved/service", version = "2" }
```

Each dependency has exactly one `path` or `host` source, optional `package` and
optional semantic version requirement. `package` defaults to the dependency key.
The key is the importing package's alias, not the dependency's identity. Package
names may contain hyphens; import aliases must be language identifiers. Keyword
aliases and conflicting installed-provider aliases are rejected.

A path is relative to the declaring manifest, within the caller's approved input
root. `..` can reach an approved sibling; it cannot escape that root. Canonical
filesystem containment and duplicate physical-source checks apply, including to
symlinks. Filesystem acquisition assumes host-controlled concurrent mutations; it
is not an OS sandbox or a transactional filesystem snapshot. Virtual trees supply the same normalized relative paths. Requirements
check the supplied package's name/version; they do not initiate a registry search.
Registry strings, Git sources, features, build scripts, macros, target selection
and package-metadata inheritance are outside the supported manifest surface.

A `host` key names an immutable `HostPackage` in the caller's explicit catalog.
It supplies source, a logical family and an exact revision. Its dependencies must
also use host keys; a virtual path/workspace layout instead uses `VirtualSources`.
The content digest covers the manifest and every supplied file. Reusing an exact
selection with different content is rejected, including against retained locks.
Host source packages do not register native Rust code. Native declarations and
runtime implementations remain separate, explicitly installed contracts.

## Workspaces

A root may contain `[workspace]` alone (a virtual workspace) or both `[workspace]`
and `[package]`. Standalone packages also work. A virtual workspace cannot declare
ordinary `[dependencies]` because it has no root package.

```toml
# game/Kagari.toml
[workspace]
namespace = "example/game"
members = ["gameplay", "rules"]
default-members = ["gameplay"]

[workspace.dependencies]
rules = { package = "combat-rules", path = "rules", version = "1" }
```

```toml
# game/gameplay/Kagari.toml
[package]
name = "gameplay"
version = "1.0.0"

[dependencies]
rules.workspace = true
```

Workspace membership supports `members` patterns, `exclude` and `default-members`.
In-workspace path dependencies join the member set unless excluded. Discover the
owning workspace through parent manifests or explicit `package.workspace`, relative
to the member manifest. A package has one owner; nested workspaces are not merged.
External members require consistent ownership and approved filesystem access.
Duplicate member names and duplicate physical manifests are errors.

Inheritance is explicit and must be exactly `{ workspace = true }`. It cannot be
combined with member-local source, package or version overrides. An inherited path
uses the workspace root as its base; a member-local path uses that member's directory.
Missing inherited entries are errors. Defining a workspace dependency neither
adds it to every package nor grants import access through workspace membership.

Default selection is the invoking member, or at the workspace root:

1. `default-members`, when declared;
2. the root package, if present;
3. all members of a virtual workspace.

Default members must belong to the resolved member set. Explicit selectors choose
packages by unambiguous member name; selecting the whole workspace builds independent
Programs. A run command requires one root. For example, `gameplay` and `simulator`
can each depend on `rules`; their Programs include their respective source closures,
not each other's functions or trait implementations. Membership creates no runtime
instance, shared heap or cross-Program atomic update boundary.

## Portable identity

A standalone acquisition root declares `package.namespace`; a workspace root
uses `workspace.namespace`. The host may supply the namespace through loading
options. If both are supplied, they must agree. Members inherit their owning
root's namespace; independent acquisition roots supply their own. There is no
fallback to cwd, an absolute checkout path, a display name or a random process ID.

The logical family is `PackageFamily { namespace, directory, name }`. `directory`
is a normalized locator relative to the namespace's acquisition root. External
members can use locators such as `../external`; the locator does not grant IO
permission. A domain-separated BLAKE3 hash of length-delimited fields becomes the
common `PackageId("source-package:<hex>")`. Moving the whole checkout preserves
identity; moving a package within its root changes its family. Same-name packages
from different namespaces/directories remain distinct nominal owners.

`PackageSelection { family, version, revision }` identifies one graph node.
`revision` distinguishes mutable local inputs from host-promised immutable revisions.
Local content changes belong to captured source fingerprints, independently of
lock identity. Runtime generations belong to individual installations, not package
versions. Matching logical identities never authorizes mixing exact selections
or bypassing runtime compatibility and ownership checks.

Modules use `ModuleIdentity { package, path }`; the root path is empty. Declaration
identity extends the owning module with its declaration path and kind. Physical
source origins and spans support diagnostics, independently of portable identity.
A source namespace cannot spoof an installed provider: source families occupy their
own identity domain, and handoff rejects provider identity/alias collisions.

## Graph resolution and locking

The shared root `Kagari.lock` covers all workspace members and all their dependencies.
Package selection determines which captured closures are checked/built, not lock
coverage. Invalid member configuration or unavailable dependencies can therefore
block a selected-member build. Unrelated source bodies remain outside its analysis.

The graph records exact selections and importer-local alias edges. Ordinary package
dependency cycles are rejected; legal module-reference cycles are unchanged. A
Program closure admits one selection per logical family and reports conflicts with
dependency chains. Different disjoint closures in the shared graph may select different
revisions. This does not enable simultaneous revisions within one Program or implement
a remote version solver. Diamond dependencies share the same exact node.

| Mode | Behavior |
| --- | --- |
| Ordinary load | Preserve available valid locked selections and resolve required configuration changes |
| `locked` | Require an existing lock and reject any required graph change |
| `offline` | Prohibit remote acquisition; local graph changes are still allowed |
| `frozen` | Combine locked and offline policy |

All currently supported sources are local or explicitly supplied. An unavailable
locked exact revision fails instead of silently selecting a replacement. A source-only
edit to a local package changes its captured fingerprint without changing the graph
lock. Manifest, selection and alias-edge changes may require a lock update. A lock
alone does not freeze mutable checkout contents. Hashes establish content identity,
not trust or native installation authority.

`ProjectInputs::load` is read-only. It returns the workspace, graph, candidate lock
and selected Program captures. `Lockfile::publish` explicitly publishes the candidate
atomically under the requested policy; cancellation/errors do not publish partial
files. CLI project commands perform that publication. Runtime APIs never do so.

`ModuleDiscovery` retains successfully captured files, all parse diagnostics and
acquisition errors. `ProjectDiscovery` resolves once and records each selected
root's capture outcome. Only a complete discovery/capture converts to `ModuleTree`
or `ProgramInputs`; `ProjectInputs::load` retains its strict all-selected-roots
behavior. Missing/ambiguous modules and physical aliases never become valid inputs.
Check/lint may report incomplete roots and continue independent complete roots,
without publishing a lock for a partially acquired request. Manifest/resolution
failure is still request-wide.

The workspace loader's `MemberDiscovery::Declared` policy selects explicit workspace
members/default-members and the root package without importing implicit local path
dependencies. Formatting uses this policy; ordinary resolution uses
`WithLocalDependencies`. This is one manifest/member loader with an explicit policy,
not a separate interpretation of package syntax. See [source-tool selection](toolchain-tools.md#commands-and-selection).

## Checking and host entries

The selected closure includes every declared dependency and every module reached
by `mod` declarations, including unused dependencies and uncalled function bodies.
Unrelated directory files are not modules. Dependencies used only for trait
implementations still participate in candidate selection and overlap checks.
Kagari's existing trait rules remain; package boundaries do not add Rust orphan rules.

Dependency aliases name only direct dependencies of the importing package. Public
re-exports can provide accessible paths to dependency items without changing their
canonical declarations. `crate`, `self` and `super` remain within their package;
`pub(crate)` permits package-wide access. [Visibility and precedence](modules.md)
apply to each component; there is no ambient source-package fallback search.

Public non-generic script free functions are callable entries, even without script
callers. Public module paths and re-export aliases are retained as a finite namespace
graph, including cycles. Generic source templates are specialized by checked source
calls; hosts use concrete non-generic script wrappers. Installed-native APIs likewise
need script wrappers for this Program export mechanism. No runtime generic
instantiation is introduced.

For example, one Program can expose `on_login`, `tick` and `damage` without three
build targets. `pub use private_impl::calculate as damage` exposes a public declaration
through a private implementation module; ordinary host lookup cannot access the
private path. Whole-Program VM/SDK entry execution selects paths from the root;
ordinary sync/async function and object-type binding instead starts at the supplied
live Module, without root fallback. See [host paths](embedding-api.md#installed-programs-and-public-paths).
`ProgramEntry::Main` separately selects
the root's non-generic zero-argument `main`; the convention grants no other private
lookup. Libraries need no `main` to check, build or serve public host calls.

Executable reachability retains all these exports. MIR and bytecode carry canonical
target identities, concrete slots and signatures; bounded source-free verification
checks them before loading. Host calls use this verified evidence, not private
bytecode display-name searches or source analysis. Calls still validate argument,
result, runtime ownership and generation contracts.

## Build inputs and updates

`ProgramInputs` captures one complete source/selection/alias closure. SDK
`KagariEngine::compile_package` atomically supplies that capture to its source
session and uses the normal checked-program/artifact pipeline. Unchanged sources
can retain file IDs/revisions and query products under their ordinary validity
contracts. Retained checked results and snapshots keep their original inputs;
separate sessions can retain independent query state for different roots.

The SDK's package input fingerprint combines:

- exact source bytes, module membership, selections and dependency aliases;
- canonical native and offline Host declaration contracts;
- parser, constant-evaluation and diagnostic limits;
- the compiler contract identifier.

Provider identities use portable records, not process allocation numbers. Physical
tooling origins and runtime callback captures are excluded. HIR owns cached provider
encoding and invalidates it on environment-setting changes. Checked results retain
the identity of their actual environment. Artifact output identity also includes
emitted bytes; input hashes alone grant no executable validity or compiler-revision
compatibility promise. Current CLI storage always checks and compiles its inputs.

`PackageBuildSet::capture` snapshots selected roots with their complete input keys.
`changes_since` reports newly selected/changed families in `rebuild` and deselected
families in `removed`. Comparing complete closures propagates dependency changes
without a second mutable reverse index. A dependency body, generic implementation,
constant, alias or version change affects its selected dependents even when callable
signatures remain unchanged. Unrelated sibling roots stay unchanged; an engine
provider/settings change conservatively affects every selected root.

The host maps these families to its installations, compiles changed inputs and
uses the existing reload API. Keep the engine environment unchanged while preparing
candidates, or compare the resulting checked key. Advance an installed baseline only
after successful publication. A removed family does not automatically unload an
installation. Multiple instances and cross-Program updates remain host-coordinated.
Reload compares frozen contracts, public paths, retained demands and local static
initializer expressions/bindings; failed candidates leave current code and state usable.
Created closures/Futures and admitted frames/queues retain exact bodies, while
ordinary inner calls and existing named bindings select current implementations.
Publication requires a host-top-level synchronous boundary; parked work may remain.
See [activation](module-activation.md).
