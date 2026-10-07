# Repository review

Keep ongoing repository review findings in this document rather than creating
dated review files. Entries record observed behavior and possible follow-ups;
they do not activate implementation work.

SA1-SA3 were inspected against `c2f87c89`. Performance impact is unmeasured.

## SA1 Inline modules are parsed again

[prepare_declarations](../crates/kagari-hir/src/analysis/declaration_queries.rs)
copies the enclosing source, masks bytes outside each inline module body and
parses that virtual file again. The parent AST already contains the module body;
[lower_module_decl](../crates/kagari-hir/src/lower/item.rs) only records its header.
This repeats lexing/parsing and allocates a full source buffer per child.

Consider lowering existing `ModuleBlock::items()` with an independent child
context. Preserve module ownership, visibility, stable child identities, original
UTF-8/CRLF offsets and diagnostic attribution. Reuse the existing inline-module
navigation tests. Keep declaration, signature and body checking separate.

## SA2 Import resolution rescans every module

[ModuleGraph::build](../crates/kagari-hir/src/imports/builder.rs) resolves all modules
against the previous catalog, rebuilds the catalog and compares it after every
round. Chained public globs need information propagation, but unrelated and stable
modules also repeat this work.

Consider a module work queue with deduplicated entries, reverse dependencies and
updates to only the affected catalog entries. Track unresolved lookups too, so
newly available names trigger retries. Preserve target, visibility and ambiguity
changes, including removals across source revisions. Where dependencies are known,
topological ordering can handle acyclic chains; strongly connected components
(SCCs) can restrict iteration to cyclic groups. Name-level propagation and query
caching are further options, not prerequisites. Measure chains, fan-out and cycles
before choosing a broader design or claiming a speedup.

## SA3 Iteration exhaustion is not distinguished from convergence

The same loop permits at most `2N + 2` rounds for `N` modules and exits early when
[catalog equality](../crates/kagari-hir/src/imports/builder.rs) finds unchanged
candidates and re-export targets. Exhausting the limit proceeds to graph construction without
an explicit non-convergence check; the code does not establish why this bound is
sufficient. No failing input has been reproduced.

Establish the bound or use a termination argument for the chosen solver, and
handle exhaustion explicitly rather than treating it as convergence. Cover long
alias/glob chains, valid cycles, unresolved cycles and cancellation.

## SA4 Import records also represent namespace lookup state

Resolved by the [IR01-IR03 execution plan](import-resolution-plan.md).
Validation and phase commits are recorded in the
[roadmap ledger](implementation-roadmap.md#import-and-namespace-resolution-ir01-ir03-complete).

The previous `ResolvedImport` model combined named bindings, glob roots, module
headers, package/prelude bindings and empty-alias namespace entries. Import-vector
positions served as declaration and namespace identities.

The implementation now separates real directives and their source provenance from
named, tiered candidates and snapshot-qualified targets. A shared
[namespace catalog](../crates/kagari-hir/src/imports/catalog.rs) resolves nested
members with importer visibility and explicit lookup states. Qualified source
units include arena identity; aliases/re-exports share canonical declarations.
Imported signature consumers, compiler handoff, navigation, dependencies and
snapshot reuse use the new boundary. Deep aliases, grouped paths, strong/glob
precedence, unused imports, stale arenas, retained snapshots and source-free/native
execution are covered by behavioral acceptance. See
[implemented ownership](architecture.md#source-namespaces-and-import-ownership).

SA1-SA3 remain separate, unactivated tasks. The solver scheduling and iteration
bound are unchanged; no performance improvement has been measured.

## SA5 Incremental reuse misses transitive namespace changes (P1)

Resolved; validation is recorded in the [SA5 follow-up ledger](implementation-roadmap.md#import-and-namespace-resolution-ir01-ir03-complete).

Reproduced against `c3294fad`. Signature reuse in
[check_signatures](../crates/kagari-hir/src/lib.rs), full-file/body reuse in
[snapshot](../crates/kagari-hir/src/analysis/mod.rs) and function-body reuse in
[body](../crates/kagari-hir/src/analysis/body_queries.rs) previously compared local
import facts and canonical imported contracts without comparing reachable namespace
bindings. Declaration reuse already performed that comparison, but later reuse
could still retain stale signatures, name resolution or checked body facts.

Setup: `library` declares the items; `exports` publicly aliases them; unchanged
`facade` contains `pub use pkg::exports::*;`; `root` imports
`use pkg::facade as m;`. Change only the aliases in `exports`:

- Rename `value as old` to `value as new`: incremental analysis still accepts
  `m::old()` and retains its navigation target; fresh analysis reports `UnknownName`.
- Swap `number as selected, flag as spare` to `flag as selected, number as spare`,
  where returns are `i32` and `bool`. Add a comment to `root` to exercise body
  remapping: incremental analysis reuses the body of
  `fn main() -> i32 { m::selected() }` and misses the return-type error.
- Swap `A as Selected, B as Spare` to `B as Selected, A as Spare`: incremental
  signature analysis still types `fn accept(x: m::Selected) {}` as `A`; fresh
  analysis returns `B`.

All affected reuse paths now compare reachable namespace bindings. Retaining a
complete file requires exact tables; reusing checked facts permits only the current
module's arena rebasing through the existing remappers, including validated local
variant owner/slot identities. Foreign namespace targets retain complete identities.
[Five regression tests](../crates/kagari-hir/src/imports/cache_tests.rs) cover the
three reproductions, single-function reuse, old snapshots, unrelated file sharing
and local nominal namespace remapping. All five tests pass, as do the HIR suite,
workspace tests completed in segments and the source-free/native feature matrix.

## SA6 Generated cache names differed from analyzed paths on Windows

Resolved at the SDK's [cache publication boundary](../crates/kagari-embed/src/engine/declarations.rs).
Windows canonicalization produced `\\?\F:\...`, which was published verbatim while
the source database normalized separators and drive spelling. The analyzed name
lost the usable drive prefix and differed from the published declaration source.
Four existing `registration_sources` tests failed their absolute-path assertion.

Publication now converts canonical drive paths to ordinary paths and applies the
same source-name normalization as analysis. The unchanged six-test suite passes,
including actual file reads, matching source views, content validation and retained
snapshot navigation. This bounded integration correction preserves all assertions.

## SA7 Routine fixes trigger expensive aggregate test suites

SA5's five focused regressions executed in 1.07s, while unrelated aggregate
language-contract, syntax-example and VM suites each took several minutes in that
validation run. These are observed suite timings, not isolated benchmarks.
Some runners serialize many scenarios inside one test and repeatedly create fresh
analysis/standard-library environments; Cargo cannot schedule those cases separately.

Routine follow-ups now use focused validation under [AGENTS.md](../AGENTS.md).
The [numeric fixture follow-up](implementation-roadmap.md#focused-test-harness-optimization-sa7-numeric-fixtures-complete)
retains all 90 cases and both artifact routes, batches 90 compilations into 4,
and installs/loads 8 runtimes instead of 180. Only scalar locals are shared within
each route; a successful call after every case checks trap cleanup. The four
numeric tests pass in an observed 4.51s; timing conditions and limitations are in
the ledger. Source-free validation and route isolation remain covered.

SDK/other aggregate runners and selectively executing their individual cases
remain separate follow-ups. Reuse immutable setup where safe and preserve tests
that specifically require fresh snapshots or independent mutable runtimes.

## SA8 Separate type/value lookup and unify export information

Design direction agreed in review: support type and value namespaces only; no
macro namespace or macro implementation. This proposal does not activate code changes.

[NameTable](../crates/kagari-hir/src/resolver/table.rs) currently keys all candidates
by `LocalName`. Declaration conflicts, imported-name selection and prelude masking
therefore share one bucket across kinds. [ImportDirective](../crates/kagari-hir/src/imports/mod.rs)
also has one resolution, although a named import may introduce both a type and a
value under the proposed rules. [Host lookup](../crates/kagari-hir/src/host.rs)
chooses functions before types. HIR `module.exports` records explicit public
declarations/imports but omits glob expansion; resolved bindings already own
visibility. These are separate issues from the solver's SA2/SA3 scheduling debt.

Recommended contract:

- Type space contains structs, enums, traits, opaque/alias types and module/package
  aliases. Generic type parameters and `Self` participate in type lookup within
  their own scopes. Value space contains functions, constants, locals, parameters
  and the existing unit/payload enum constructors. Keep canonical declaration IDs;
  classify bindings, not the declarations' identity format.
- Permit cross-space names; reject conflicting definitions/imports within each
  space. Apply `strong > glob > implicit` separately in each space. Values must
  not hide prelude types. Update the current broad shadowing language in the specs.
- Resolve path prefixes through the type/module space, then select the terminal
  space from the use site. `T { ... }` and struct patterns select a type; calls
  and enum constructor patterns select values. Associated types/constants/methods
  use the appropriate terminal category while retaining type-checker ownership of
  trait/inherent dispatch. Field and method access on values remains separate.
- A named `use m::Name as Local` introduces every applicable type/value binding
  under `Local`; keep one source directive and per-space outcomes. Distinguish
  pending, absent, resolved, inaccessible and ambiguous outcomes. Absence in one
  space is valid if another succeeds; conflicts/access failures remain explicit.
  Pending explicit imports must prevent premature glob/prelude selection in the
  affected space; proven absence releases that reservation. Globs enumerate both
  spaces, preserving per-binding visibility, precedence, provenance and dependencies.

Suggested storage (pseudocode; retain the existing candidate representation):

```rust
enum NameNamespace { Type, Value }
struct PerNamespace<T> { types: T, values: T }
// NameEntry retains strong/globs/implicit candidate lists.
type NameEntries = BTreeMap<LocalName, PerNamespace<NameEntry>>;
// Named outcomes are per-space; glob resolution identifies its target namespace.
enum ImportResolution {
    Named(PerNamespace<LookupResult>),
    Glob(NamespaceLookupResult),
}
```

Require explicit categories on semantic `lookup`, `lookup_member` and path APIs;
provide a deliberate two-space operation for imports and tooling. Do not retain
an unqualified first-match fallback. `NamespaceId` continues to identify a module
or associated-member container; it is not the new type/value category. Replace
HIR `alias + alias_explicit + glob` with explicit named/glob syntax, deriving the
default local name from the final path segment. Remove redundant HIR `Export`
storage after migrating its actual consumers (including variant handling); derive
public-member views from resolved bindings for both named and glob re-exports.

Execution order for a future plan:

1. Specify category membership, constructor/pattern rules, same-space conflicts,
   import outcomes and prelude shadowing in the module/syntax specs.
2. Migrate declaration collection, name tables, catalog lookup, import resolution
   and host lookup/registration validation. Audit canonical identity consumers;
   preserve identity, visibility and source-free boundaries. Distinguish source
   names from native linkage symbols; do not relax ABI symbol uniqueness.
3. Migrate type/body/pattern/associated-member queries, imported contracts and
   export consumers. Update navigation/completion/reference provenance and cache
   comparison/remapping together; include category and both import outcomes.
4. Use a small focused contract matrix: cross-space coexistence versus same-space
   conflicts; aliases/globs/re-exports of both spaces; constructors and patterns;
   host/prelude lookup; incremental add/remove of one category versus fresh analysis,
   retaining old snapshots. Reuse existing tests; full suites belong to GitHub CI.

Keep SA2/SA3 as a later checkpoint: a dependency work queue and explicit convergence
handling should follow stable lookup semantics. No hash-map replacement or speedup
claim is justified by this review. Rust references for the boundaries:
[binding keys](https://doc.rust-lang.org/stable/nightly-rustc/rustc_resolve/struct.BindingKey.html),
[per-name state](https://doc.rust-lang.org/stable/nightly-rustc/rustc_resolve/imports/struct.NameResolution.html),
[import resolution and derived module children](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_resolve/imports.rs.html).

## SA9 Module paths are copied into internal graph keys and references

[ModuleIdentity](../crates/kagari-common/src/identity.rs) is a portable package/path
value containing owned strings. It is also embedded in `SourceUnit`, binding owners,
namespace keys and dependency sets. Cloning these identities copies strings; hashing
and ordering examine path content. The fixed-point builder clones catalog keys on
each round. This establishes representation overhead, not a measured bottleneck.

Separate portable module identity from a compact, context-owned module handle.
Intern each identity once; use handles for catalog keys, owners and dependencies,
and borrow the portable identity for diagnostics, registration and artifact boundaries.
The existing [definition table](../crates/kagari-common/src/identity/table.rs) already
interns module roots and shares their paths; investigate extending/reusing that
owner before introducing a second interner. Do not reuse HIR `ModuleId`: it addresses
a local module declaration, not a module in the analysis universe.

The design must define handle ownership and retained-snapshot validity, remap across
independent contexts, retain source revision/arena checks, and preserve deterministic
artifact ordering independently of allocation order. Parent/package/visibility checks
need table queries in place of editing copied path vectors. `Arc<ModuleIdentity>`
alone reduces clone costs but still compares/hashes content with ordinary traits.

Rust reference:

- The resolver's [`Module<'ra>`](https://doc.rust-lang.org/stable/nightly-rustc/rustc_resolve/struct.Module.html)
  wraps `Interned<'ra, ModuleData<'ra>>`: a `Copy` handle with identity-based
  equality, while module data is stored separately. It does not copy a full
  string path into every module reference.
- Definitions, including modules, use [`DefId`](https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/def_id/struct.DefId.html),
  a `Copy` pair of `CrateNum` and `DefIndex`, for compiler queries.
- Session-local numeric IDs can change after source edits. The
  [incremental cache](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
  stores `DefPathHash` and remaps it to the current session's `DefId` when loading.

Apply the same separation in Kagari: portable `ModuleIdentity` at boundaries,
shared identity data in the context, compact handles inside the graph. This does
not require introducing Rust's crate model or adopting its arena lifetimes.

Keep this a separate follow-up from SA8 and SA2/SA3. Measure graph build allocations
and time before/after on the same module workload, separating compilation time;
reuse identity/snapshot and import tests for focused validation. Code migration is
not activated by this review.
