# Package and Dependency Design

Status: active implementation, PK00-PK04 authorized on 2026-10-14. Kagari needs a package boundary for
source organization, imports, dependencies and reproducible builds, without making
package distribution or filesystem access part of VM execution. This design uses
Cargo-style dependency declarations and separates package resolution from runtime
publication. The companion [update model](update-model-design.md) owns compatible
hot reload and state replacement.

The user approved adopting the Rust/Cargo rules explicitly listed below as the
baseline. Source distribution with joint compilation, one root module per package
with multiple callable exports, workspace management, and the toolchain/Rust
distribution names are confirmed design decisions. Kagari-specific differences and
implementation contracts are recorded below. The initial delivery uses `Kagari.toml`,
`Kagari.lock`, `src/lib.kgr`, local paths and explicitly supplied host packages. Each
Program admits one selection per logical family. The progress ledger distinguishes
implemented inputs from pending compiler/runtime integration; publication is not authorized.

## Current foundation

- [PackageId and ModuleIdentity](../crates/kagari-common/src/identity.rs) already
  separate a string package identity from a module path. They do not constitute a
  package manifest, version resolver or lockfile.
- [Module semantics](spec/modules.md) support registered source modules, imports,
  visibility and dependency closures. Analysis does not discover unregistered files.
  Module cycles are allowed; `pub(crate)` is implemented in the first PK03 checkpoint.
- [Loading specifications](spec/module-loading.md) describe host-approved package
  roots and dependency mappings, and reserve `kg` for future tooling. They are not
  evidence of an implemented general package manager.
- General packages use common identities and source handoff; native declaration
  installation is a separate boundary, not an implicit package privilege.
- Script workspace discovery and selection are implemented in `kagari-package`;
  whole-workspace graph locking and immutable Program input capture are implemented;
  compiler integration remains pending. The repository's Cargo
  workspace groups Kagari's Rust implementation crates, not user script packages.

Re-audit these foundations at activation. Existing specifications govern until
an implementation updates them.
Preserve [source-module execution tests](../crates/kagari-embed/tests/source_modules/execution.rs)
and [source snapshots](../crates/kagari-embed/tests/source_snapshots.rs) when changing
resolution and input registration; they are not full package-manager acceptance.

## Responsibilities

| Concept | Meaning |
| --- | --- |
| Workspace | Development group of packages with shared configuration, lockfile and build storage |
| Package | Named source/interface unit with a root module and declared dependencies |
| Module | Namespace within one package |
| Resolved package graph | Exact package selections and dependency-alias edges for one build |
| Program | Prepared executable closure selected from that graph |
| Script | Runtime-local logical installation of a Program under the host API proposal |

A package is not a runtime heap and is not automatically an atomic update unit.
One Program can contain code from multiple packages; multiple Script installations
can independently use the same package. Building a library does not run an entry
function. Running a tool or script remains an explicit host/CLI function call.

Confirmed initial model: each script package has one root module, which organizes
its module tree and public API. A package may export multiple callable functions;
its root source file is not an execution entry. For example, a host can invoke
`on_login`, `tick` and `simulate` from the same prepared Program without creating
three build targets. Each call still checks public access, argument/result contracts
and runtime/generation ownership.

The initial script package model has no separate library or executable targets.
The host explicitly selects a function to invoke. CLI execution retains the existing
`main` convention; an explicit function selector and its argument syntax remain
queued CLI design choices. These script package rules do not alter the Rust SDK/CLI
package distribution described below. Build scripts, procedural macros, feature
unification and platform dependency selection also remain outside the initial
scope. Unsupported manifest fields must be diagnosed, not ignored.

## Adopted Rust/Cargo baseline

Use these rules as implementation requirements, not open alternatives to redesign:

| Concern | Adopted rule | Kagari adaptation |
| --- | --- | --- |
| Module tree | An unnamed root, child modules and root-relative paths | One source root per package; migrate root identity through analysis and executable validation |
| Visibility | Private, `pub`, `pub(super)`, `pub(crate)` and public re-exports | Preserve canonical declarations; retain checked public callable paths in Program artifacts |
| Dependency names | Each package binds its direct dependencies under local aliases | Alias maps and exact selections are immutable analysis inputs |
| Workspace | Membership, explicit inheritance and package selection | Normalize before HIR; selected roots produce separate Programs |
| Locking | One workspace resolution graph, shared lockfile and distinct locked/offline behavior | Extract a closure per Program; retain source snapshots separately |

The following sections define the supported subset and its owner. Full Cargo
configuration compatibility, Rust's complete orphan rules, independently linked
crate binaries and Rust compiler identity encodings are not implied. Host entry
metadata and generation-pinned reload remain Kagari contracts.

## Toolchain CLI and execution modes

### Confirmed naming and Rust distribution

The product is Kagari. Its two user-facing Rust packages serve distinct workflows:

| Role | Target package or command | User workflow |
| --- | --- | --- |
| Embedding SDK on crates.io | `kagari` | `cargo add kagari`; Rust imports use `kagari::...` |
| Installable toolchain package on crates.io | `kagari-cli` | `cargo install kagari-cli --locked` |
| Executable installed by the CLI package | `kg` | `kg check`, `kg run`, `kg repl` |
| Implementation components | `kagari-runtime`, `kagari-hir`, etc. | Dependencies behind the SDK and tooling boundaries |

These are target functional-release workflows; the published reservation versions
below do not provide them. The main `kagari` package owns the embedding SDK because embedding is a
primary product purpose; users should not need to select between runtime and VM
implementation crates to find that SDK. Keep the CLI package separate so argument
parsing, terminal interaction and package acquisition do not become SDK dependencies.
A package name and its executable target can differ; see
[Cargo binary targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#binaries)
and [Cargo installation](https://doc.rust-lang.org/cargo/commands/cargo-install.html).

When the separately queued distribution naming change is implemented, rename the
existing `kagari-embed` package and directory directly to `kagari`, updating consumers
and documentation. Keep `kagari-cli` as the package
and directory name, and declare its executable as `kg`. Replace the current binary
entrypoint rather than retaining a second compatibility command. Do not add an
SDK forwarding package, a legacy `kagari-embed` alias or a separate `kg` package.
The [Host API design](host-api-refactor.md) continues to own SDK API evolution;
this naming decision does not require completing that refactor first.

crates.io distributes the Rust SDK, CLI and their required Rust component packages.
It is separate from the future registry or acquisition mechanisms for Kagari script
packages. Before publishing functional releases, confirm name ownership for all
required components, supply registry
versions alongside local workspace paths, and publish the required dependency
closure. Cargo does not bundle sibling workspace packages into the CLI archive;
see [path and registry dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations).
Do not publish unrelated workspace tools merely because they share the repository.
Release automation, functional-release version policy and publication of the actual
SDK/toolchain remain deferred. Workspace manifests and versions remain unchanged.

After user review and explicit publication approval, standalone, dependency-free
reservation packages were published on 2026-10-12:

- [`kagari` 0.0.0-reserved](https://crates.io/crates/kagari/0.0.0-reserved).
- [`kagari-cli` 0.0.0-reserved](https://crates.io/crates/kagari-cli/0.0.0-reserved).

The SDK placeholder exposes no API; the CLI's `kg` binary identifies the placeholder
and rejects script commands. Both link to the actual development repository and
carry the project licenses. This reserves the two package names without migrating
the SDK or releasing the toolchain. Future functional versions replace these
placeholders directly; no compatibility API or forwarding package is introduced.
The published archives contain only manifests/lockfiles, README, project licenses
and minimal source. Package builds and CLI help/version/error behavior passed local
checks; registry SHA-256 checksums matched the reviewed archives after publication.
These results cover only the reservation versions, not SDK/toolchain acceptance.

### Command responsibilities

`kg` is the unified development toolchain entrypoint, including package management.
The command vocabulary below is the target design, not the current CLI help:

| Command | Intended responsibility |
| --- | --- |
| `kg check [file.kgr]` | Check the selected project or file and its source dependency closure without emitting executable code |
| `kg fmt [file.kgr]` | Format the selected project sources or explicit file; `--check` reports differences without writing |
| `kg lint [file.kgr]` | Run additional code-quality rules using syntax and available semantic facts |
| `kg build` | Check the selected project and emit its executable artifact |
| `kg run [file.kgr]` | Build and invoke the selected project entry or explicit script entry |
| `kg repl` | Start a persistent interactive session |

CLI code owns argument parsing, filesystem/terminal IO, input selection and command
orchestration. Package loading prepares source snapshots, dependencies and host
contract inputs. Formatting uses lossless syntax; linting reuses analysis queries;
checking reuses HIR semantics; executable generation and execution retain their
existing compiler and runtime owners. Formatting needs source selection, not
semantic dependency resolution or downloads. Checking and semantic linting share
the same declared environment as building.

The existing `compile_source`/`compile_snapshot` SDK path already ends at checked
HIR; `emit_bytecode` performs MIR and bytecode generation separately. Reuse that
boundary. A successful check covers the selected source rules, including bodies
not called by the entrypoint; it does not promise successful later code generation
or execution. Language errors cannot be disabled as optional lint rules.

### Project, standalone script and REPL inputs

A project supplies its manifest, source modules, declared dependencies and entry
selection. A standalone script needs no on-disk manifest: tooling supplies a
host-owned in-memory package identity and uses the same checking, compilation and
execution pipeline. `kg run file.kgr` does not itself authorize a new language rule
allowing top-level statements; initially reuse the existing function-entry model.

REPL design requires a persistent analysis/runtime session, multiline input and
retained bindings/results with explicit GC ownership. For example, after accepting
`val x = 10;`, a later input `x + 2` should display `12`. Invalid syntax or types
must not commit a new input to the session. A runtime trap preserves completed
side effects under the existing language contract; session persistence is not a
transactional rollback mechanism. Reuse ordinary analysis, execution, cancellation,
roots and generation-pinned call rules rather than a second interpreter.

Before activating these modes, settle project discovery for explicit files,
standalone dependency/host declarations, entry selection and argument forwarding,
and REPL expression results and declaration lifetimes. The proposed initial REPL
policy rejects duplicate declarations and offers `:reset`; redefinition syntax and
reload integration remain open. Missing dependencies must produce diagnostics,
not ambient cross-project lookup or undeclared downloads. CLI naming is confirmed;
these detailed mode policies and their implementation sequence remain queued.

## Manifest and dependency declarations

The manifest and lock filenames are `Kagari.toml` and `Kagari.lock`. The shape borrows Cargo's
package metadata and dependency tables; exact compatibility with Cargo files is not
promised. Cargo supports registry/version, path and Git dependencies, including
dependency renaming. These inform the declaration model, not VM loading behavior.
See [Cargo dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)
and [manifest structure](https://doc.rust-lang.org/cargo/reference/manifest.html).

```toml
[package]
name = "gameplay"
namespace = "example/gameplay"
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

Initial delivery: path dependencies and host-supplied resolved packages,
with a lock/graph contract capable of carrying future Git/registry selections.
Remote fetching, registry publication and a full multi-version solver can follow.
This staging does not imply that Cargo only supports these features. A host dependency
uses `{ package = "service-api", host = "approved/service", version = "2" }`; the key
must occur in the caller-supplied immutable input collection. Version constraints
validate supplied inputs; they do not initiate remote lookup.

## Workspaces

Confirmed initial scope: follow Cargo's workspace organization for script packages.
Support a root manifest containing both `[workspace]` and `[package]`, and a virtual
workspace manifest containing `[workspace]` without a root package. Standalone
packages remain supported. See [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
for the reference model; Kagari does not promise full Cargo manifest compatibility.

```toml
# game/Kagari.toml
[workspace]
namespace = "example/game"
members = ["gameplay", "combat", "shared"]
default-members = ["gameplay"]

[workspace.dependencies]
combat = { path = "combat" }
shared = { path = "shared" }
```

```toml
# game/gameplay/Kagari.toml
[package]
name = "gameplay"
version = "0.1.0"

[dependencies]
combat.workspace = true
shared.workspace = true
```

The initial workspace surface includes:

- `members`, `exclude` and `default-members` for membership and default selection.
- `[workspace.dependencies]` with explicit member inheritance through
  `workspace = true`. A workspace dependency does not become a member's dependency
  until that member declares it; workspace membership itself grants no import access.
- A shared root `Kagari.lock` and build cache/output directory, with products keyed
  by the existing complete build fingerprints rather than only a member name.
- Package selection such as `kg check --workspace` and `kg build -p gameplay`.
  CLI spellings here are target behavior, not implemented commands.

Resolve paths in workspace dependency declarations against the workspace root;
resolve paths written directly in a member manifest against that member's directory.
Normalization retains the declaring manifest/site for diagnostics. Missing inherited
entries and incompatible declaration combinations produce errors instead of an
ambient package lookup. Default members must belong to the resolved member set.

Adopt Cargo-style discovery and selection: member patterns support globs;
in-workspace path dependencies join the member set unless excluded. Locate the
owning root through parent manifests or an explicit `package.workspace` path.
A package has one owning workspace; do not merge nested workspaces. External
members still require consistent ownership and host-approved access.

From a member, select that package by default. At the workspace root, use
`default-members`, otherwise the root package or all virtual-workspace members.
Support explicit package selection and reject ambiguous selectors. A run operation
must identify one package and function before execution; multi-package builds do
not run functions. These rules follow [Cargo package selection](https://doc.rust-lang.org/cargo/reference/workspaces.html#package-selection).

Workspace discovery and configuration inheritance belong to package tooling.
Normalize each member into an ordinary package description with explicit dependency
requirements before dependency resolution. HIR and compilation receive the same
resolved graph/source snapshot contract used for standalone and virtual packages;
runtime does not interpret workspace configuration. Filesystem discovery remains
subject to host-approved roots. Workspace location is not a substitute for portable
package identity.

For example, if `gameplay` and `simulator` each depend on `combat`, building
`gameplay` compiles its source closure into one Program; building `simulator`
produces another. Building both selects two roots, not one combined language world.
Analysis/generated products may be reused only under their ordinary validity rules;
unrelated members do not contribute names or trait implementations to a Program.
Workspace membership introduces no runtime instance, shared mutable state or atomic
multi-Program reload boundary.

The shared lockfile stores one resolved graph covering all workspace members and
their dependencies, not separate independently solved root graphs. Package selection
chooses which closures to check/build; it does not narrow lock coverage. Invalid
member manifests or unresolved dependencies can therefore block resolution for a
selected-member build. Unselected, unrelated source bodies and trait implementations
remain outside that Program's semantic environment.

Exact selected-package nodes and dependency edges preserve distinct revisions when
present in the graph. Extract each selected root's closure before applying the
single-revision-per-family restriction. The workspace-wide graph may hold
several revisions used by disjoint Program closures; this does not enable multiple
revisions within one Program or implement a remote version solver. See
[Cargo resolution](https://doc.rust-lang.org/cargo/reference/resolver.html) for the
workspace-wide resolution model.

Workspace package-metadata inheritance, features, build scripts and broader Cargo
configuration are outside this initial workspace scope. Unsupported configuration
must be diagnosed under the ordinary manifest rules.

## Resolution and locking

Separate requirements from selections: the manifest describes requirements while
the resolved graph/lock records exact choices. This follows the distinction in
[Cargo's manifest and lockfile guide](https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html).

Resolution belongs to host tooling before compilation. Runtime import/call/reload
must not perform network requests, update a lock or select a newer dependency.
Adopt the distinction between [Cargo's locked and offline options](https://doc.rust-lang.org/cargo/commands/cargo-build.html#manifest-options):

| Operation | Contract for supported source kinds |
| --- | --- |
| Ordinary check/build | Reuse valid locked selections; resolve required manifest changes and atomically write the resulting whole-workspace lock |
| `--locked` | Require an existing lock and reject any required lock change; do not silently select replacement dependencies |
| `--offline` | Prohibit network access; local resolution and lock updates remain possible |
| `--frozen` | Combine locked and offline behavior |
| Explicit dependency update | Reconsider requested selections and necessary affected edges while retaining unrelated valid selections; exact inputs must be available |

Unavailable exact selections fail rather than being replaced behind a locked
request. A source-only edit to a path package changes the build snapshot/fingerprint
without necessarily changing the lock. Manifest, source identity or dependency-edge
changes can require a lock update. Source kinds not implemented in the initial
scope remain errors in every mode; these rules do not activate remote acquisition.
The explicit update command spelling is outside PK's initial CLI acceptance.

Lock records should identify canonical source, package name, selected version,
exact remote revision/checksum where applicable and resolved dependency edges.
Mutable path/virtual sources additionally produce a captured build snapshot and
content digest. A lockfile alone does not freeze the contents of a local checkout.
Deployment publishes the exact resolved graph and build fingerprints, not just a
manifest with version ranges. Hashes establish integrity/cache identity, not trust
or authorization to install Native implementations.

Initial restriction: select one revision of each canonical package
family per Program graph and reject conflicting requirements with a dependency
chain diagnostic. Supporting simultaneous versions is a later resolver/type-identity
extension. This restriction does not forbid old and new runtime generations from
coexisting during compatible reload. Independent Programs may use different
selections from the shared workspace graph; separate workspaces may resolve
independently. Do not solve each workspace root again after reading the shared lock.

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

Adopt Rust's [unnamed crate-root model](https://doc.rust-lang.org/reference/crates-and-source-files.html).
The proposed physical root is `src/lib.kgr`, overridable in the manifest. Its target
identity is `ModuleIdentity { package, path: [] }`; `mod combat` extends that path
to `["combat"]`. The root filename is not a namespace component. `crate::` starts
at the empty path, `self::` at the current module and `super::` at its parent;
traversal above the root is an error.

This is a migration, not a description of current code: source registration rejects
empty module paths, import normalization keeps the first path segment for `crate`,
and executable layout/native-import validation has nonempty-path assumptions.
PK00 inventories these consumers; PK03 replaces the assumptions coherently, including
single-file synthetic packages and source-free validation. Distinct single-file
roots must not collapse when the former filename path segment is removed. Preserve
all remaining identity, bounds and owner checks; no compatibility root alias is needed.

Module declarations select child sources: `foo.kgr` or `foo/mod.kgr` can represent
the same child path but both together are an error. Reuse syntax parsing for discovery,
including external children of inline modules; do not create a second grammar or
make HIR perform IO. The loader registers approved sources in an immutable snapshot.
Virtual packages supply the equivalent root and module map.

A dependency alias such as `rules::` resolves through the importing package's direct
dependency map to an exact selected package root. Transitive dependencies are not
implicitly named; public re-exports can expose their accessible items. A missing
direct dependency is diagnosed rather than found through a global package-name
fallback. The current global installed-alias input must be replaced with the
importer-relative environment, including explicit standard/host package bindings.

Reject conflicting dependency aliases, reserved roots and ambiguous bindings with
clear diagnostics. Existing local/import precedence must remain explicit; package
loading must not introduce a hidden fallback search order. Validate every module
and item visibility boundary through the existing resolver.

Adopt Rust's [visibility and re-export rules](https://doc.rust-lang.org/reference/visibility-and-privacy.html)
for the supported forms: retain private, `pub` and `pub(super)`, and add `pub(crate)`
for package-wide access. Check every component of a direct path. A public re-export
can provide an accessible route through a private implementation module to a public
item without changing that item's declaration identity. This does not widen a
private item's declared access. `pub(in path)` remains outside the initial scope.
`pub(crate)` is planned language work, not an existing implementation capability.

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

Confirmed initial model: script packages are distributed as source and compiled
together with their resolved dependencies. Compilation selects a root's package
closure from the resolved graph and produces a Program with exact provenance.
Keep three scopes explicit:

- Manifests determine package dependencies, independently of `use` statements.
- Each selected package root and its `mod` declarations determine the source module
  tree to check, including uncalled bodies. Unrelated directory files are not modules.
- Script calls and retained host exports determine executable requirements, including
  generic instances. An unused import is not a package-selection mechanism.

Within a package, analysis uses its own module tree and dependency environment;
reverse dependents and unrelated workspace members do not add implementation
candidates. Preserve Kagari's existing trait matching and conservative overlap
checks, including rejecting conflicting implementations in the complete Program
closure. Do not introduce Rust orphan rules as part of package work. PK03 must
replace import-edge-only preparation where it cannot represent these package scopes.

Each exact Program selection gets an isolated analysis input environment. The
initial implementation may use separate source snapshots/database forks for roots
with different package revisions; it must not register two revisions under one
logical module identity in the same graph. Dependency aliases, exact selections,
source revisions and standard/host declarations participate in cache validity even
when source text is unchanged. Retained snapshots keep their old environments.

Compilation must retain the functions exposed for host invocation and their
execution dependencies, even when no script function calls them. Checked public
paths map to canonical declarations and concrete executable targets; for example,
`pub use internal::calculate as damage` retains `damage` as an accessible path to
the original declaration. Retain this mapping in the Program/artifact so source-free
host lookup does not search private bytecode names or rerun HIR resolution. Linking
validates targets/signatures and reload validates the public callable contract.

Treat retained exports as executable reachability roots; selecting one function
for a call does not discard other exports. PK00 specifies the exact export-selection
policy, artifact representation and concrete generic entry requests before code
generation. A public generic declaration alone does not materialize every possible
instantiation. The CLI's existing non-public `main` convention remains an explicit
entry policy, not permission for ordinary host lookup to bypass public access.

The initial scope excludes independently compiled per-package binary libraries and
a cross-package binary linker. Generic specialization uses the source closure in
the existing compilation pipeline; it does not require a separately distributed
library template format. The resulting complete Program can still be serialized
and loaded without source under the existing artifact validation contracts.

Internal compilation caches may reuse checked or generated products, but are not a
package distribution format or a public binary-linking contract. Cache keys include
dependency and host-contract fingerprints, not just source paths or package version
strings. Joint compilation does not require recompiling unchanged inputs on every
build. This decision does not select a local or remote source acquisition mechanism.

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

The [adopted baseline](#adopted-rustcargo-baseline), single-root/multiple-export
model, source distribution/joint compilation and workspace-wide lock graph are
confirmed. Implement their stated rules rather than reopening Rust/Cargo defaults.
Multiple executable targets and independent package binary distribution/linking
remain outside scope.

The initial filenames, default root, local/host acquisition subset and
single-selection-per-family Program restriction are frozen below. Ordinary package dependency cycles are rejected;
existing legal module cycles within a package remain supported. Adding `pub(crate)`
is part of the adopted visibility subset.

PK00 owns Kagari-specific contracts: portable logical family assignment for local
and virtual packages, exact selection/input ownership, empty-root identity migration,
host export metadata and concrete generic entry policy. Record the wire/input
representations and affected validation owners, not a second progress queue. The
[Host API plan](host-api-refactor.md) owns invocation facade evolution and the
[update design](update-model-design.md) owns compatibility/state replacement; PK
supplies identities, export contracts and affected-root information without requiring
those complete facade migrations. The roadmap continues to own activation order.

Package acceptance covers checking/building/running selected projects and workspace
members through the existing SDK/compiler/runtime boundaries. REPL, formatting,
optional lint commands, SDK/package renaming and functional publication retain their
separate queued scope; do not pull them into PK03 through generic "CLI integration".

- [x] PK00: Freeze the Kagari-specific contracts above and remaining initial subset
  choices. Map existing nonempty-root, global-alias and import-closure consumers;
  select focused tests and artifact changes. Exit with concrete input/export models,
  migration owners and acceptance examples, not just a manifest sketch.
- [x] PK01: Normalize package/workspace manifests and virtual inputs using the adopted
  discovery, selection and inheritance rules. Produce deterministic module maps and
  dependency requirements with diagnostic origins. Test path bases, membership,
  conflicting definitions and virtual/path equivalence without invoking the VM.
- [x] PK02: Resolve the complete workspace graph and atomically maintain its lock;
  capture source snapshots and extract exact Program closures. Test locked/offline
  distinctions, preserved unrelated selections and closure conflicts with supported
  local/host sources. No remote solver is required.
- [ ] PK03: Integrate in buildable checkpoints. First pass a virtual resolved graph
  through root/visibility/alias migration, checked package scopes, artifact export
  metadata and multiple host calls. Then connect disk/workspace selection, CLI and
  shared build storage to that same path. Finally connect reverse-dependency
  invalidation and existing reload, checking source-free calls and pinned old code.
  Select focused tests at each boundary; preserve source-free production dependencies.
- [ ] PK04: Update specifications/examples and close the selected package/workspace
  acceptance cases. Leave the full feature/backend matrix to CI. Do not mark
  unexecuted CI acceptance complete; remote providers remain separate scope.

## PK00 frozen contracts and migration owners

The user selected a source namespace declared at the project root, also supplyable
by a host. A standalone root uses `package.namespace`; a workspace root uses
`workspace.namespace`. Supplying both host and manifest values requires equality.
Member packages inherit the root namespace and do not establish new namespaces.
An external independent acquisition root supplies its own namespace. There is no
fallback to cwd, an absolute checkout path, random process ID or display name.

`PackageFamily { namespace, directory, name }` owns portable identity. `directory`
is normalized relative to that namespace's acquisition root. A domain-separated
BLAKE3 hash of length-delimited fields becomes `PackageId("source-package:<hex>")`.
Moving the whole root preserves identity; moving a package within it changes its
family. Explicit external members retain a normalized relative locator such as
`../external`; this logical locator does not authorize IO outside the approved root.
Installed provider identity collisions are rejected during handoff; choosing
a source namespace never installs native APIs or confers provider privileges.

`PackageSelection { family, version, revision }` is the graph node identity.
`revision` distinguishes mutable local input from a host-promised immutable revision.
Local source bytes have a separate content fingerprint, so ordinary source edits
invalidate compilation without rewriting `Kagari.lock`. Host revisions must identify
exact supplied content; reusing a revision with different content is an error.

`kagari-package` owns acquisition, strict TOML, workspace discovery, graph/lock
resolution and immutable source capture. It depends on common/source/syntax and
never on executable runtimes. The whole-workspace graph records exact selections
and importer-local alias edges. A selected Program input contains one root, its
complete dependency closure, declared module trees and source/graph fingerprints.
An immutable source-owned package environment carries only the normalized families,
direct aliases and module membership to HIR; HIR does not read manifests or disks.
Each selected Program gets its own environment, never a union of incompatible roots.

The source root becomes `ModuleIdentity { package, path: [] }`. Module discovery
follows `mod` declarations, supports `foo.kgr` and `foo/mod.kgr`, and rejects both
existing for the same declaration. Inline `mod foo { mod bar; }` searches beneath
`foo/`. Files not declared in the module tree are not compiled. A package closure
includes every declared dependency and every declared module, including uncalled
bodies and implementation-only dependencies. Membership adds no import privilege.
Module cycles retain existing semantics; package dependency cycles are rejected.

Program exports are checked public paths relative to the root package, mapped to
a canonical declaration and a concrete `(module, function)` executable target.
For example, `pub use crate::internal::calculate as damage;` records `damage` as
the callable path and keeps `calculate` as the target declaration. Public child
paths are traversable; private paths and re-export ambiguity are rejected by HIR.
Initially export non-generic public script free functions, including ones without script
callers. Generic and installed-native APIs use ordinary non-generic script wrappers
for host calls; source consumers may import and specialize public generic templates.
No runtime generic instantiation is introduced. Store public module aliases as a
finite namespace graph with independent module/function name slots, rather than
expanding cycles into infinitely many strings. A host path follows these checked
edges under the identity path-length bound, ending at a canonical concrete function.
CLI `main` is a separately recorded private entry capability, not a general
private-function lookup exception.

Bytecode construction, codec and source-free verification retain and bound the
export map, validate target slots/canonical identities/concrete call contracts,
and reject duplicate paths or mismatched targets before publication. Runtime and
SDK lookup consume verified export evidence rather than scanning private names.
Reload compares the public-path contract and retains the existing pinned generations.
No format/ABI version bump is needed for these unpublished development changes.

| Migration boundary | Current consumer and required change | Focused evidence |
| --- | --- | --- |
| Source identities | `kagari-source/src/source_database.rs` rejects empty module paths; single-file roots currently share the synthetic `source` family | Empty-root binding, distinct standalone identities and retained source snapshots |
| Import environments | `kagari-hir/src/imports/{catalog,builder,resolve}.rs` owns global aliases, `crate` truncation and implicit native edges | Importer-specific alias changes, `crate/self/super`, missing direct dependencies and provider isolation |
| Analysis scope | HIR analysis/declaration/signature queries, `aggregates` and `program.rs` currently follow module import closures | Unused body errors, implementation-only dependencies, unrelated-member isolation and trait conflicts |
| Visibility | Syntax visibility/parser and `kagari-types/src/visibility.rs` | `pub(crate)` and root/private/re-export access |
| Executable identity | `kagari-contract/src/{layout,native_import}.rs` reject empty roots; compiler ABI and Program lowering enumerate local functions | Root types, canonical re-export targets and uncalled public exports |
| Product and execution | Bytecode Program construction/codec/verification, VM function lookup, runtime function handles and SDK typed calls | Source-free exported calls, malformed export rejection, typed signatures and pinned reload |
| Tooling and invalidation | SDK source sessions/snapshots and CLI selection | Disk/virtual equivalence, workspace selection, dependency-only/alias-only changes and affected-root reporting |

## Implementation ledger

- 2026-10-14: PK00 complete. Reviewed current source/import/analysis/lowering,
  executable validation, SDK/CLI and existing snapshot/reload tests. Froze the
  contracts above after the user's namespace decision. Activated only PK00-PK04;
  SDK renaming, remote acquisition, broader tooling and publication remain queued.
- 2026-10-14: PK01 complete. `kagari-package` normalizes strict bounded manifests,
  explicit dependency inheritance/path bases, virtual and bounded filesystem inputs,
  portable family identity and declared inline/external module trees. Workspace
  discovery covers roots, virtual workspaces, member/default/exclude globs, explicit
  external association, parent discovery, nested-root rejection, auto-members and
  duplicate physical manifests. Namespace disagreements fail before input publication.
  Filesystem canonicalization assumes host-controlled concurrent mutations; this
  acquisition policy is not an OS sandbox.
- PK01 validation: 17 distinct tests passed across `--test inputs` and
  `--test workspace`, including disk/virtual relocation, unselected manifest errors,
  directory/module limits and cancellation. The final two physical-alias tests were
  run with the `physical` filter after a Windows test-helper repair. Windows requires
  privilege for true symlinks, so local tests use directory junctions to prove escape
  rejection and duplicate canonical source/manifest detection. Unix symlink coverage
  remains CI-only. Scoped all-targets Clippy, structural scan, formatting and diff
  checks passed. No whole-workspace Cargo suite was run.
- PK01 review repairs: recursive glob depth exhaustion now fails instead of returning
  partial membership. Clippy's `manual_contains` was fixed. Initial physical-link
  fixtures failed with Windows error 123 because Windows PowerShell's junction cmdlet
  cannot consume verbatim `\\?\` targets; normalize only the test helper's arguments
  and retain canonical paths in production. Both affected cases then passed.
- 2026-10-14: PK02 complete. Resolve the entire workspace graph with bounded
  package/edge counts, source-name/version checks, cycle paths and one-selection-per-family
  Program closures. Mutable local selections retain content separately; immutable
  host selections verify their manifest/file fingerprint against both the input
  catalog and existing lock. Valid locked host revisions survive preferred-provider
  and local importer-version changes; unavailable exact input fails without silently
  upgrading. Different revisions are allowed across disjoint Program closures.
- Lock format 1 uses deterministic TOML selections, member locators, alias edges,
  host keys and immutable-content hashes. Decoding rejects duplicates, dangling/cyclic
  edges, inconsistent source kinds, unknown fields and oversized input. Local/offline
  publication writes and syncs a same-directory temporary file, then replaces the
  lock; unchanged locks are not rewritten. Locked/frozen modes prohibit changes.
  Concurrent filesystem mutation remains host-controlled; this is atomic file
  publication, not a filesystem snapshot or a workspace-wide runtime transaction.
- PK02 validation: 11 focused `--test graph` cases passed, covering whole-workspace
  versus selected-body scope, locks/flags, exact-input integrity and availability,
  alias-only/source-only changes, retained old inputs, diamonds, same-name sources,
  conflicting/disjoint revisions, malformed locks, cancellation, atomic publication
  and relocation-stable locks/fingerprints. Rechecked the cycle test after improving
  its dependency-chain diagnostic. Package Clippy, structure, formatting and diff
  checks passed. The prior 17 input/workspace cases remain evidence for unchanged
  normalization/discovery behavior; no broad Cargo sweep was run.
- PK03 in progress: implemented `pub(crate)` parsing, AST/HIR lowering, package-wide
  access and re-export restrictions. Moved re-export scope containment into the
  shared types visibility owner so access and export checks use one scope model.
  Extended the existing source-free field/method artifact tests; package-restricted
  members retain their metadata scope without entering public method export tables.
- PK03 visibility validation: one parser, six HIR visibility and six compiler
  visibility/contract tests passed. These include sibling access, foreign-package
  rejection, forbidden public re-exports, metadata mismatch rejection and retained
  signature invalidation. The full structural scan passed; specification links and
  package TOML examples checked. Selected-library Clippy, formatting and diff checks
  passed. No other source/artifact/backend suites were run.
- PK03 source integration checkpoint: `ProgramInputs::source_snapshot` now publishes
  complete captured files and importer-specific scopes through one cancellable,
  atomic source database batch. Stable logical keys are separate from diagnostic
  origins; unchanged files retain IDs/revisions and removed selections leave the new
  snapshot. HIR alone derives inline modules. Standalone sources have distinct
  families and every package root uses `path: []`; executable declaration/layout
  validators accept empty roots while retaining component/ownership checks.
- Source import lookup now uses explicit per-importer dependency aliases and
  structured `crate/self/super` paths. Arbitrary registered source packages are no
  longer implicit aliases. Installed native aliases are separately owned and source
  collisions fail before publication. Package hubs include every declared module and
  dependency in checking/trait selection without an all-to-all member edge graph.
  Direct qualified calls project signatures even without `use`; retained namespace,
  alias/selection, type and callable comparisons prevent stale incremental facts.
- PK03 focused source validation: six package/HIR integration tests cover distinct
  aliases for same-named packages, transitive import rejection, unused body errors,
  empty roots/inline modules, retained snapshots, alias-only replacement and
  implementation-only dependency selection/conflicts versus unrelated members.
  Six source database tests cover atomic invalid/cancelled batches and existing
  overlays/locations. The HIR import owner passed 47 tests (one opt-in measurement
  ignored); 42 signature-filtered tests passed after separating signature namespace
  comparison from body/callable revision comparison. These sets overlap. One existing
  compiler source-program linking/portable-codec test and six visibility/artifact
  verification tests passed, covering root types and source-free contracts.
- Review repairs: older isolation fixtures incorrectly placed unrelated or deliberately
  invalid modules in the same package. Bind unrelated files as independent packages;
  check valid package snapshots before adding invalid members. Preserve all error,
  retained-snapshot and cache-reuse assertions. A cross-database nominal-type test
  now imports both values into one identity context before comparing them. The
  1024-node cancellation test now exercises graph construction/traversal directly;
  its initial body-analysis run was interrupted and the completed focused graph
  rerun passed. Initial two needless-reference Clippy findings were corrected;
  affected-library Clippy, the structural scan and formatting/diff checks pass.
- PK03 export discovery checkpoint: checked Programs retain public module edges
  and canonical concrete script function leaves. A bounded namespace graph preserves
  cyclic aliases without infinite flattened paths and keeps Type/module and Value
  spellings independent. Private `main` is a separate zero-argument entry capability;
  generic/native APIs require concrete script wrappers for host calls.
- The previous public-generic rejection mixed source visibility with runtime entry
  policy. Remove it from both declaration/call checking and public ABI validation,
  preserving ordinary binder/type/bound/native-authority checks. Source dependents
  can specialize public generic functions; executable host export filtering remains
  concrete-only. Initial focused failures exposed the obsolete HIR diagnostic and
  then `InvalidPublicAbi`; the corrected cross-module alias/specialization test now
  passes through MIR and bytecode verification. The public namespace/entry test also
  passes. The existing SDK generic-entry case now exercises a public dependency
  template through its concrete caller across source/artifact and JIT/fallback
  routes; it passes. The native generic bound/registration-authority regression
  also passes, as does the source-free required/forged-native declaration rejection
  test. The existing cyclic module/re-export regression also passes. All seven
  package-analysis tests pass with export collection enabled.
  Scoped HIR/contract/SDK library Clippy, structure, formatting and diff checks pass.
  Executable export persistence and runtime public lookup remain next; no complete
  backend/feature matrix or whole-workspace Cargo suite was run.
- PK03 MIR export checkpoint: contract-owned namespace records retain canonical
  definition IDs independently of source analysis. `MirProgram` now owns root, modules
  and exports together; editing/import no longer drops Program metadata. The MIR
  codec encodes explicit file-local references, bounds records and rejects duplicate
  namespace/name records. Verification checks graph reachability, concrete public
  script bodies and exact semantic signatures; private zero-argument root `main`
  remains a separate capability. No format/ABI bump or compatibility reader is added.
- MIR export validation: the two focused source-free export tests pass, including
  canonical re-encoding, cross-context ID adoption, alias cycles, independent name
  spaces, private/template path rejection and the separate entry. The forged-graph
  case was strengthened to require `InvalidExports` for seven distinct invalid
  inputs, then rerun successfully. The contract wire test rejects duplicate
  namespaces/names, invalid references and oversized sequence headers. The existing
  five-source MIR codec/canonical-bytecode regression and the source generic alias
  specialization test pass. The existing forged-control-flow MIR decoder test also
  passes. Scoped contract/MIR/compiler library Clippy, the 1143-file structural scan,
  formatting and diff checks pass. These are focused tests, not full backend
  acceptance; bytecode/runtime export consumption and exhaustive CI remain pending.
- PK03 bytecode export checkpoint: Program facts and canonical artifact bytes now
  retain the shared export graph. Source-free verification checks public/entry
  contracts and binds canonical declarations to exact module/function slots. The
  immutable bytecode seal retains these bindings together with suspension evidence.
  Runtime normalization remaps graph/binding IDs together while keeping the original
  Program version; loaded public lookup stays rooted at the Program even through
  member handles. Hand-authored raw fixtures explicitly expose no paths unless
  they supply export facts; source compilation carries its checked graph.
- Export validation uses a minimal source fixture without unrelated stdlib bodies.
  The compiler tests cover MIR/KBC roundtrips, ID adoption, aliases/cycles and seven
  malformed-graph/signature cases. The first bytecode fixture incorrectly replaced
  whole ABI records and lacked its normal identity-collection setup; restrict edits
  to the intended bytecode fields and prepare identities through their owner. The
  existing bytecode semantic-flow checker catches signature mismatch before export
  binding, so that case asserts its precise earlier error. Other malformed graphs
  retain `InvalidProgramExports` assertions. Both compiler export tests pass.
  Runtime import/rooted-lookup and retained-generation coverage also passes; compare
  the old/new constant-pool values rather than expecting their slot instructions to
  differ. VM execution routes and public-path reload compatibility remain pending.
- Bytecode checkpoint validation: the native-preparation owner rejects independently
  valid bytecode whose public alias differs from its portable MIR, in addition to
  its existing body/origin mismatch cases. The artifact module-count preflight test
  passes. Contract/MIR/bytecode/compiler/runtime library Clippy and the 1146-file
  structural scan pass; formatting and diff checks pass. The initial compile error
  imported `FunctionRef` through a private bytecode import; use its contract owner.
  No carried compile/test failure; exhaustive backend/features/platform CI is unrun.
- PK03 public reload checkpoint: all verified-runtime, raw-bytecode and artifact
  candidate routes now compare public namespace/function paths plus the optional
  root entry. Existing module ABI/path checks retain their order and guarantees.
  Alias retargeting is compatible when public names and semantic signatures match;
  nominal types retain their exact canonical identity. Import signatures into one
  temporary context before comparison. Union matching namespace representatives
  instead of expanding cyclic paths or traversing a quadratic graph product.
- Focused reload evidence: runtime package tests cover equivalent one/two-state
  cycles, compatible alias retargeting, retained old targets, rejected public name
  changes, scalar/nominal signature changes and entry removal/result changes. Raw
  bytecode and source-free artifact candidates enforce the same result across
  independent identity scopes. A declared entry must carry a semantic result;
  the expanded compiler malformed-export case verifies omission rejection in MIR
  and the existing earlier bytecode semantic-flow checker. Initial test fixtures
  attempted to publicly re-export private modules; declare those modules public
  as required by source visibility. No visibility relaxation was introduced.
- Local reload checkpoint validation: runtime package exports (3 tests), existing
  runtime `reload_rejects_` ABI/stale-epoch regressions (2 tests), and the compiler
  malformed-export test pass. Runtime library check and runtime/contract library
  Clippy pass. Structure scans 1,147 Rust files with no violations; formatting and
  diff checks pass. Full workspace/backend/feature/platform CI remains unrun.
- PK03 typed function binding checkpoint: `Runtime::bind_function` resolves Program-root
  public paths through verified export slots, including aliases into other modules.
  Binding uses the target module for semantic type conversion and retains the original
  Program through the existing typed function cache/lease. Explicit public declaration
  and application APIs keep their checked native/generic evidence path. Remove the
  obsolete local-name generic shortcut rather than preserve a second name resolver.
- Expanded the existing source-free closure/function pinning regression with a private
  implementation module, public aliases, a cyclic root alias and child-handle lookup.
  It rejects private paths and private `main`, exercises aliases through reload and
  GC, and retains the old closure/target. The old local-name implementation failed on
  the alias as expected. Generic native coverage retains explicit declaration binding
  and private/missing-application rejection; ordinary names cannot bypass exports.
- Typed binding validation: `cargo test -p kagari-vm --test native_boundary
  native_boundary_functions:: --no-fail-fast` passes all 40 selected callable-boundary
  cases (121 unrelated tests filtered out), covering the shared preparation path,
  native/generic/member evidence, GC, cancellation, candidate isolation and reload.
  Runtime library Clippy, structure (1,147 files), formatting and diff checks pass.
  No carried failures. Full workspace/backend/feature/platform CI remains unrun.
- PK03 direct entry checkpoint: VM ordinary/typed/owned/prepared execution and SDK
  native preparation select verified export slots through `ProgramEntry`. Strings
  mean public paths; `Main` is the separate zero-argument root capability. The CLI
  selects `Main` explicitly. Native compilation/cache/install matching and typed
  conversion use the resolved member, preserving version correspondence. Removed
  private display-name scanning and its obsolete runtime ambiguity error.
- Source callers use explicit `Main` for script execution and public declarations
  for other host entries. The typed native example exposes an argument-taking
  `split_text` export; the async task example exposes `handle` and `ping`. Some
  dynamic test helpers and low-level hand-authored bytecode fixtures still require
  review; this checkpoint does not claim complete consumer migration.
- Direct-entry evidence: VM `--test package_exports` passes source-free public
  aliases/cycles, typed arguments, owned driving, fallback and private/generic path
  rejection on root/child handles. The VM conformance case
  `public_entries_use_verified_identity_despite_duplicate_display_names` passes
  direct/artifact/fallback lookup despite duplicate implementation display names.
  SDK `--test native_preparation
  caches_products_across_runtimes_and_retains_them_after_backend_drop` passes with
  re-exported targets, target-module assertions, private `main` rejection and
  prepared-target mismatch rejection. Runtime/VM/SDK library Clippy passes. CLI
  `--features jit` check and `emits_and_runs_kbc_artifacts_through_cli_pipeline`
  pass. The four SDK dynamic-helper test targets `never`, `artifact_features`,
  `async_execution` and `syntax_examples` compile; their runtime migration audit
  remains pending. Structure scans 1,149 files without violations; formatting and
  diff checks pass.
- Carried test failure, owned by the immediate PK03 consumer follow-up:
  `cargo test -p kagari-vm --lib executes_runtime_reflect_type_of_helper` fails with
  `MissingFunction("main")`. Its hand-authored bytecode deliberately has no export
  graph, canonical function identity or closed source signature. Audit other raw
  fixtures in VM `tests/common`, `execution`, `helpers`, `jit`, `installed_access`
  and `native_wait`: retain their actual opcode/host/GC/debugger assertions and use
  explicit low-level executable references or correctly authored export contracts;
  never restore a production name-scan fallback or fabricate semantic evidence.
- Consumer follow-up also audits dynamic entry helpers in SDK `never`,
  `artifact_features`, `operator_traits`, `iteration_traits`, `syntax_examples`,
  `async_execution`, and VM native-boundary storage/interfaces. Literal named-entry
  source declarations were migrated; dynamically supplied names need corresponding
  public declarations or explicit root-entry selection. Preserve trap classifications
  so missing-entry errors cannot falsely satisfy tests expecting script failures.
- Next: finish that finite entry-consumer migration and its affected checks, then
  SDK/CLI/shared storage and invalidation/reload. Package capture is connected to HIR,
  but SDK and CLI do not yet expose it. Native-provider inputs must contribute their
  own contract fingerprint at compiler handoff. The carried raw-fixture failure is
  not final acceptance. Full workspace/backend/feature/platform CI remains unrun;
  no whole-workspace Cargo suite was run locally.
- Intermediate compile diagnostic: initial module-discovery code treated optional
  syntax diagnostic spans as mandatory. Corrected by preserving `Option<Range>`;
  the input-test build passed. No carried build failure from this diagnostic.
- Validation scope: package input tests cover normalization and module acquisition
  without VM startup. Later PK03 selects the existing source snapshot and dependency
  execution/reload owners plus distinct export/source-free cases. Whole-workspace
  Cargo suites, full feature/backend/platform coverage and CI acceptance are unrun.

## Acceptance and verification

Cover same-named packages from different sources, dependency aliases, missing direct
dependencies, diamond graphs/conflicts, cycles, duplicate module paths, virtual/path
equivalence, checkout relocation and standard/host namespace spoofing. Test lock
reproducibility, local source changes, unavailable locked input, path traversal and
symlink policy, cancellation, bounded graph size and atomic publication of inputs.

Cover root-package and virtual workspaces, standalone packages, member/exclusion
and default selection, inherited versus member-local path bases, missing inherited
dependencies and the adopted whole-workspace lock rules. Cover lock generation
while building one member, missing/stale locks, source-only path edits under
`--locked`, offline lock updates and unavailable exact inputs. Verify that sibling
membership grants neither import access nor trait candidates, and that multiple
selected members yield independent Programs with correctly keyed products.

Cover an empty-path root through source and artifact routes, `crate/self/super`,
root-level types, package aliases targeting different packages, and alias-only
input changes. Keep old snapshots while checking new selections; exercise disjoint
root closures selecting different revisions without co-registering conflicting
logical modules. Cover declared dependencies without `use`, implementation-only
dependencies and whole-closure implementation conflicts under the existing trait
rules. These are distinct from executable dead-code/reachability checks.

Verify that multiple exported functions without script callers remain independently
callable from one Program, including after source-free artifact loading, under the
selected export policy. Cover public re-export aliases, public child paths, private
path rejection and the CLI entry exception. Cover the selected concrete generic
entry policy without assuming arbitrary runtime instantiation.

Run end-to-end single-file and multi-package source/artifact routes, including
changes to a dependency function, generic body and constant. Verify affected roots
are identified, unchanged code can be reused without stale behavior, and no package
fetch or initialization occurs at runtime. Keep the existing source-free/backend
feature matrix in CI. Local checkpoints use focused tests for changed package,
workspace and directly affected source/artifact/reload contracts, plus lightweight
structure, formatting and diff checks as applicable. Record the selected tests and
remaining CI scope during implementation; documentation-only changes need content,
link and diff checks. The complete sequence below is CI-only:

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
