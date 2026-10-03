# Kagari Implementation Roadmap

## Scoped definition identities (in progress, 2026-10-03)

The user authorized the Rust-inspired definition-table migration. ID01-ID05 own
the complete native declaration/HIR/ABI/MIR/bytecode/artifact/runtime identity
chain. Preserve exact package/module/kind/name/occurrence identity at boundaries;
use table-scoped Copy IDs internally. Do not introduce stable path hashes,
incremental disk caching, declaration macros, old-format readers, ABI/version
bumps, foundation registration redesign or interpreter-loop optimization.

- [x] ID01: record the owned-identity baseline; implement checked table primitives,
  shared snapshots, name interning and explicit cross-table remapping.
- [ ] ID02: migrate declarations, analysis/cache ownership and compiled metadata.
- [ ] ID03: migrate explicit portable table encoding, canonical fingerprints,
  bounded decoding and source-free validation; regenerate affected fixtures once.
- [ ] ID04: migrate native linking, codecs, type environments, layouts and reload
  identity remapping while retaining executable generations and immutable seals.
- [ ] ID05: remove transitional models; accept the full integration matrix and
  record allocation, metadata-size, artifact-size and timing evidence.

Design: a short ID contains private u32 table and node indices (8 bytes); table
numbers are process-local, checked and never reused. Roots hold logical module
identities; child nodes hold parent/kind/interned-name/occurrence. Builders append
without changing existing indices; immutable snapshots share owned metadata.
Each native module, analysis snapshot and verified program retains its table;
runtime linking maps exact identities into a runtime-owned context once per
preparation. Cross-table equality requires explicit import. A declaration ID does
not replace HIR arena/body identity or module epoch/generation checks.

Portable records contain canonical identity tables and local references, never
process table numbers. Encoding/decoding receives its table explicitly. Fingerprint
inputs use canonical exact identity content, independent of table insertion order
and unrelated entries. Reject foreign/out-of-range references, duplicate identities,
invalid parent/string references, cycles, paths over 64 segments and tables over
1,000,000 records before executable adoption; retain existing envelope/type limits.

ID02-ID04 form a continuous migration window: record intermediate diagnostics and
their owning follow-up; commit coherent buildable checkpoints, with no compatibility
facades or disabled validation. Final acceptance includes structure, fmt, strict
workspace/all-target Clippy, workspace tests, complete language contracts, standalone
feature/backend consumers and diff checks. Use Roadmap-Step: ID01 through ID05.

Ledger: starts from clean 7857fd8a; no carried build or validation error. Inspection
finds 124 Rust files mentioning the owned DefinitionId. Native modules are authored
independently, and immutable verified programs can link into multiple runtimes;
explicit contextual import is required at both boundaries. Short-ID copies must
allocate nothing; measured speedups are not assumed.

ID01: identity table primitives retain immutable Arc snapshots and share interned
module/name payloads. Explicit imports include each referenced ancestor once and
reject foreign/unmapped IDs. Portable table encoding orders exact identities and
omits unrelated entries; decoding rejects duplicate identities/names/modules,
forward/cyclic parent edges, missing references and path depth violations. Count
prefixes are bounded before reading table elements. The common crate passes all
33 tests (10 new identity-table tests). Focused all-target strict Clippy and the
structure checker (663 Rust files, zero violations/exceptions) pass. The standalone
representation probe records 72-byte owned headers and 8-byte short IDs; 100,000
copies require 600,000 versus zero allocations. See the performance baseline for
methodology and timing limits. Existing consumers intentionally still use owned
DefinitionId; ScopedDefinitionId is the temporary short-type name until ID02
replaces the owned model. No production runtime speedup or end-to-end migration
is claimed by ID01, and ID02-ID05 remain required. No carried error.

ID02 entry checkpoint: rename the owned portable representation to DefinitionPath
through all 128 affected Rust files (including ID01 files), and give the eight-byte
table-owned type its final DefinitionId name at identity::table. No aliases or
re-exports preserve the old API. This is API terminology separation, not adoption
of compact IDs by executable metadata: GenericParameterAbi, nominal ABI/HIR types,
native registrations and runtime type environments still carry DefinitionPath.
The remaining ID02 ownership/remapping work and ID03-ID05 are not accepted. The
workspace/all-target check and full workspace regression suite pass, including the
unchanged complete language-contract matrix (80.26 seconds; not a paired speedup
measurement), source-free artifact execution, backend and pinned-reload coverage.
Use a NonZeroU32 table discriminator so both DefinitionId and Option<DefinitionId>
occupy eight bytes; the final focused common rerun passes all 33 tests. Strict
workspace/all-target Clippy, fmt, structure (663 files, zero findings/exceptions)
and diff checks pass. Portable fixture/source correspondence still passes without
regeneration because this checkpoint changes Rust API names, not encoded layouts.
The independent feature-consumer matrix remains an ID05 integration requirement;
no carried build/test error remains. This checkpoint does not complete ID02 or
the full execution plan. Breaking Rust API change: import DefinitionPath for
owned path construction and identity::table::DefinitionId for compact contextual
references; the transitional ScopedDefinitionId name is removed.

ID02 adoption checkpoint: local BindingId body owners now use scoped Copy IDs;
each analysis retains the checked definition table. Native declaration catalogs,
binding registries and storage indexes retain eight-byte keys in a shared explicit
context. Context clones share one append-only writer rather than forking numeric
indices; staged installation still publishes entries only after contract checks.
Cross-catalog merge/subset/exclusion operations import exact identities once and
avoid copying owned key paths. Existing complete-contract equality, proof-count
bounds, independent-runtime installation and failed-batch behavior are retained.
Unreferenced interned names confer no registration authority.

AbiType, NominalAbiType, GenericParameterAbi, generic bounds and constraints now
parameterize identity representation instead of duplicating semantic models.
Bounded substitution and flat type codecs share their implementation for paths,
scoped IDs and local portable references. The explicit PortableTypes codec covers
all owner/member/output references, rejects foreign/invalid/collapsed references,
preserves argument order and produces canonical bytes despite scope/insertion
order differences. It is a type-set codec, not yet the KBC/MIR outer codec.
Runtime TypeEnvironment stores compact binder owners in the runtime's context,
retains parent/generation scope and uses the same checked substitution through
contextual lookup instead of rebuilding owned-path substitution maps. Reject
foreign-context parents and preserve types-only metadata after owner drop.

The representation probe records binder headers 80 -> 16 bytes, ABI type headers
128 -> 64 bytes and nominal headers 120 -> 56 bytes. Only frame binder/index
storage is adopted so far; existing compiled nominal/type records still instantiate
the path representation. These sizes do not establish artifact or execution gains.
Full workspace regression passes (complete language contracts: 80.70 seconds,
unpaired observation). Focused common/native tests and all 49 ABI tests pass;
the final source-identity boundary test rejects paths above 64 segments without a
panic and passes separately after integration. SourceDatabase binding now enforces
the same bound, while analysis recovery reports CompileLimitExceeded and rejects
codegen for oversized externally constructed inline sources. Initial generic scalar
inference diagnostics are resolved through ValueType::from_builtin_type; an
experimental global error conversion is removed rather than broadening unrelated
inference sites. The analysis body-cache index now shares an explicit contextual
table and stores short keys; source-facing owner paths remain query locators.
All 401 HIR tests and 35 common tests plus the bare-ID serialization compile-fail
test pass after this change. Final formatting, strict workspace/all-target Clippy,
structure (669 files, zero violations/exceptions) and diff checks pass.

ID02-ID05 remain required: migrate named/generic HIR ownership;
propagate the identity parameter through declaration/callable/layout/MIR/bytecode
records and adopt scoped checked products; replace outer artifact/fingerprint codecs
and regenerate fixtures at that boundary; finish codecs/layout/closure/reload
normalization and independent feature/backend consumers; measure complete metadata,
artifact and timing effects. No phase-completion claim is made by this checkpoint.

ID02/ID03 executable-schema checkpoint: declaration, host-interface, callable,
layout, MIR, bytecode and artifact records now share one identity-parameterized
model. Ordinary module-owned traversal implementations cover every identity field;
record constructors make additions visible during compilation. Mapping rejects
collapsed dictionary/set keys and observes cancellation. A shared metadata owner
retains an immutable checked scope; it does not claim semantic verification.
Canonical portable projections retain only referenced definitions and ancestors,
and decoding resolves every local reference before semantic adoption.

KBC and MIR codecs now encode exact tables plus local references without changing
unpublished format/ABI version identifiers or accepting old layouts. The existing
64 MiB envelopes, preflight count limits, bounded flat types, complete verification,
debug origins, float bits, source-free behavior and canonical correspondence remain.
VerifiedBytecodeProgram retains only scoped records and its immutable table, with
no serialized evidence or mutation API. Explicit mutable extraction materializes
authoring paths and discards the seal. Native-input correspondence compares complete
canonical portable projections rather than process-local IDs or compatibility hashes.

Focused common/ABI/MIR/bytecode regressions pass (including three new contextual
metadata tests); all ten compiler codec tests pass. Two forged-MIR fixtures initially
used the replaced layout and failed before their intended verifier boundary. Their
construction now uses the shared portable projection; the original control-flow
rejection and aggregate analysis-budget assertions remain unchanged and pass.
The standalone artifact-only/source/native/source-native feature consumers and
eight production crate-boundary checks pass after coherent disposable fixture
regeneration. Strict workspace/all-target Clippy and structure (694 files, zero
violations/exceptions), formatting and diff checks pass. Full workspace regression
passes with no carried build, codec or verification error.

ID04 runtime adoption checkpoint: frame/object/layout/loaded-module, native-call
and VM records now instantiate scoped metadata. Runtime VerifiedProgram retains
compact modules, their owning table and the immutable original version. Preparation
imports exact identities once into the runtime context without replacing version
identity, dependency versions or epochs. Host type bindings are resolved once during
linking. Type arguments retain their own immutable table after their runtime drops;
foreign tables remain rejected. Named execution queries borrow table views rather
than materializing paths. Fingerprints, authoring contract checks and explicit mutable
extraction remain exact-path boundaries. Contextual validation uses the same bounded
type algorithm as authoring validation, including nominal kinds and associated members.

All 124 carried runtime diagnostics are resolved. Workspace/all-target checking and
strict Clippy pass. Runtime/VM/embed library and integration regressions pass, including
complete language contracts (75.96 seconds, unpaired), source-free execution, native
backends and pinned reload. Focused common/ABI tests pass (39/50), followed by runtime
native execution, layout and host registration tests and embed host-interface/offline
nominal tests. New boundary coverage checks foreign references, independent runtime
imports, unchanged immutable version identity, reload epochs and retained type tables.
Structure checks cover 695 Rust files with no violations or exceptions. No carried
build, codec, verification or test error remains. Full final integration and measured
normalization costs remain ID05 work; these observations do not establish a speedup.

ID02/ID04 MIR ownership checkpoint: immutable module/program seals now retain scoped
records and a checked definition table; a dependency program shares one scope.
Verification uses ephemeral authoring input until all module and closure checks
complete, then retains the existing physical analysis facts with compact records.
Cranelift and bytecode lowering consume these checked records. Named ABI methods
are interned from their checked declarations, including unused marker traits;
unreferenced names confer no authority and are omitted from portable projections.
Explicit mutable extraction discards the seal; cancellable extraction is used by
optimization passes. Default-contract proof checking and unsealed public bytecode
output remain explicit authoring boundaries, without retaining expanded function
bodies in backend input. Identity builders enforce the one-million-node adoption
limit as well as portable decoding limits.

Workspace/all-target checking, compiler/MIR/Cranelift regressions and structure
(696 Rust files, zero findings/exceptions) pass. Compiler regression covers 161
unit tests, four foundation and ten source-program tests. The codec scope regression
also checks independent table identities, foreign-reference rejection, shared closure
scope, retained names after program drop and extraction cancellation. Its first
retention assertion selected an empty native module; selecting a module containing
a script function fixes that test setup, and the focused rerun passes. The initial
compiler interface diagnostics are resolved; no carried build/test error remains.

ID02-ID05 are still not accepted: named/generic HIR ownership and native catalog
value metadata still retain authoring paths. Finish those
ownership boundaries before the final feature/backend matrix and metadata/artifact/
allocation/timing measurements. The parameterized authoring model is an explicit
input boundary, not a second semantic implementation or a compatibility reader.

## Kagari/Lua execution diagnosis (completed, 2026-10-03)

BP02 owns diagnosis of BP01's large measured execution gap. Add a checked,
execution-only sampling mode, independent Kagari/Lua instruction counting and
GC statistics. Keep production semantics and implementations unchanged; runtime
optimization belongs to a subsequent authorized task. Commit with
`Roadmap-Step: BP02`.

- [x] Profile all six nontrivial fixtures with optimized private Rust symbols.
- [x] Separate sampling, instruction-count hooks, setup and compilation.
- [x] Record dominant paths, object/instruction counts and sampling limitations.
- [x] Accept strict Clippy, formatting, structure, benchmark regression and diff checks.

Ledger: starts from clean a07da95c. The release/debug=2 diagnostic build preserves
optimization level 3; the driver sets only its child build environment and saves
an executable/PDB pair. Six ten-second windows yield 25,491 instruction-pointer
samples, zero context errors and checked results. Arithmetic spends 23.21% of
samples in frame access/checks, 10.03% in termination-state lookup and 12.10% in
instruction fetch/clone. Its 750,014 instructions compare with Lua's 250,007;
no GC occurs in scalar or call fixtures. Maps allocate 2,001 GC objects per
call, including 2,000 Option results, and collect five times. See the
[diagnostic methodology/results](../benchmarks/lua-comparison/README.md#interpreter-diagnosis-bp02-2026-10-03).
System CPU recording was unavailable; local target-thread sampling needs no ETW
privileges. An initial minimal-symbol run was discarded after detecting adjacent
public-symbol misattribution; private Rust PDBs resolve the reported hot paths.
No recorded speedup or production optimization belongs to this checkpoint.
Strict workspace/all-target Clippy, formatting, structure (659 Rust files, zero
violations/exceptions), the empty/single-element regression, ordinary release
smoke checks across all fifteen routes, Python syntax/document links and diff
checks pass. The initial diagnostic sort-style Clippy finding is resolved; no
compilation or validation error is carried. Production behavior is unchanged,
so the full language-contract matrix is not rerun.

## Matched Kagari/Lua benchmark (completed, 2026-10-03)

BP01 owns a reproducible comparison requested by the user: standard Lua 5.4
against Kagari's interpreter and actual supported JIT routes, covering arithmetic,
branching, script calls/recursion and collections. Use matching algorithms and
inputs, independent result checks and separate setup/execution measurements.
Language grammar, safety checks and container implementations remain distinct;
no runtime optimization or broader backend implementation belongs to this task.
Commit this checkpoint with `Roadmap-Step: BP01`.

- [x] Add paired source fixtures and an isolated benchmark-only Lua dependency.
- [x] Check default, empty and single-element inputs against independent results.
- [x] Measure two sequential processes with alternating/reversed execution order.
- [x] Record machine, profile, features, cache conditions, raw data and limitations.
- [x] Accept strict Clippy, formatting, structure, diff and production boundaries.

Ledger: starts from clean 45aa927d. Seven paired fixtures cover fifteen actual
routes: seven VM, seven Lua and one supported native entry. All 330 measured
batches pass; each route has 22 warm execution samples. Six larger workloads
show VM/Lua median ratios of 104.71–220.44 in this baseline; JIT rejects their
script-call entries rather than timing an interpreter fallback. Setup and build
durations are recorded separately. See the [benchmark methodology and results](../benchmarks/lua-comparison/README.md).
The benchmark regression verifies all seven fixtures with zero and one as inputs.
Strict workspace/all-target Clippy, formatting, structure (658 Rust files, zero
violations/exceptions), diff and eight production dependency boundaries plus
the ABI build graph pass. Production implementations are unchanged; the full
language contract matrix is not rerun. No carried compilation or validation
failure remains. Raw output and metadata are under target/lua-comparison.

## Workspace item spacing (completed, 2026-10-03)

- [x] Separate adjacent Rust functions, type definitions and implementation blocks throughout the repository.
- [x] Preserve attributes, documentation attachment, macro tokens and embedded source literals.
- [x] Accept formatting, structure and whitespace-only diff checks.

Ledger: the user expanded the declaration readability cleanup from one file to
the entire project. Starting from clean 8e3d5195, syntax-aware inspection of all
656 Rust files inserts 1,195 blank separators in 224 files, including tests,
examples, review tools and macro bodies. Every edited source retains identical
syntax leaf tokens and nonblank lines; a second spacing scan finds no remaining
candidate. `cargo fmt --all -- --check`, the structure checker (656 files, zero
violations/exceptions) and `git diff --check` pass. The Rust diff disappears with
`--ignore-blank-lines`. No compilation or behavioral test rerun is needed for
this whitespace-only checkpoint; no error is carried.

## Verified preparation reuse (completed, 2026-10-03)

TO02 owns R6's measured repeated bytecode verification. Introduce an immutable,
resource-bounded bytecode verification seal in the bytecode crate; consuming
artifact validation retains that seal for frontend-free native correspondence
and runtime adoption. Native correspondence still decodes/verifies MIR, bounds
the lowered result and compares every canonical byte with the sealed bytecode
before any execution. Preserve fresh external-input/envelope validation and
all runtime-local host/native linking, roots, bounds and generation checks.
No artifact/ABI version, test route, profile, foundation reachability or generic
trait-cache migration belongs to this checkpoint. Commit with `Roadmap-Step: TO02`.

- [x] Retain immutable bytecode verification across artifact/native/runtime preparation.
- [x] Reject modified/forged code, mismatched portable input and resource exhaustion.
- [x] Measure the complete language-contract matrix before/after, excluding builds.
- [x] Accept workspace, structure, formatting, Clippy and feature consumers.

Ledger: starts from clean 145b34b1. The user authorized R6 after the complete
149-case/408-route diagnosis. 1,030 bytecode verifications took 65.282 seconds
in that instrumented standalone O1 run. The bounded implementation reuses the
whole graph's proof evidence rather than introducing independently cached trait
search results. No build/test error is carried at task start.

TO02 implementation retains a bytecode-owned immutable seal after consuming full
artifact validation. Frontend-free native correspondence requires that seal;
canonical lowering is internal and unsealed until complete byte equality proves
it identical to verified code. Runtime adopts the seal without repeated graph
checking. Native preparation changes from four graph checks to one; opaque MIR
in artifact-only/source-only builds and runtime-local linking remain unchanged.
The obsolete preparation Runtime error variant is removed because runtime seal
adoption is infallible. No compatibility aliases or version changes are added.

Focused bytecode verification passes 29 tests and two compile-fail seal tests;
nine compiler codec tests pass, including invalid code, independent valid payload
mismatch, changed source origins, NaN payload/float-bit differences and cancellation.
The first codec attempt failed because the new fixture changed a pool constant
without its matching instruction operands (`MissingConstant`); correcting both
keeps the graph independently valid and verifies native correspondence rejection.
That fixture failure is resolved. No production validation failure is carried.

SDK native/artifact focused tests pass all ten cases. Structure checks cover 656
Rust files with zero violations/exceptions; formatting and diff checks pass.
The paired standalone, uninstrumented full language-contract matrix passes before
and after: 155.36 -> 105.71 seconds (32.0% reduction), excluding
Rust compilation. All 149 cases and 408 routes remain. See the
[TO02 performance record](performance-baseline.md#verified-preparation-reuse-to02-2026-10-03).

The first workspace integration attempt found CLI E0599 at main.rs:415: its
preparation-error printer still matched the removed Runtime variant. The CLI
consumer is updated to the current enum; this carried compilation failure is
owned and resolved by TO02. The completed workspace and CLI-feature reruns pass.

Final acceptance: `cargo test --workspace --no-fail-fast` passes 1,590 tests with
zero failures and one existing ignored manual sorting measurement. Its complete
language-contract matrix passes in 95.40 seconds; this workspace observation is
separate from the paired standalone timing. Strict workspace/all-target Clippy,
formatting, structure (656 Rust files, no violations/exceptions), diff and all 66
changed-document file links pass. Standalone artifact-only, source, native and
source+native consumers pass, including eight production dependency boundaries
and the ABI build graph. CLI jit-feature tests pass all five cases. TO02 is
accepted with no carried error. Logs are under ignored target/verification-reuse.
Raw compiler emission and direct runtime artifact/reload entrypoints retain their
independent validation; this checkpoint does not introduce a global trait cache
or change test-matrix parallelism.

## Complete language-contract diagnosis (completed, 2026-10-03)

The [complete profile](performance-baseline.md#complete-language-contract-profile-after-to01-2026-10-03)
starts from clean bcde912e and retains all assertions/routes. An isolated prebuilt
default source/native O1 test passes 149 cases (102 executable, 47 diagnostic)
and 408 routes in 135.72 seconds, excluding Rust compilation. Direct/encoded
preparation accounts for 49.2%, source analysis/lowering/artifact construction
26.5%, and fresh-runtime construction 23.0%. The 1,030 bytecode verification
passes nested in compilation/preparation take 65.282 seconds (48.1% overall).
A separate passing scalar probe locates about 95% of its bytecode verification
time in per-module trait-bound checking, including repeated proof catalogs and
dependency obligations. Script/native execution is small for this workload.

All five temporary Rust instrumentation changes are restored byte-for-byte;
document file links and diff checks pass. No production change or carried error
remains. This diagnosis supports R6 as the next bounded optimization candidate
but does not activate it or test-matrix restructuring. The earlier 327.88-second
workspace observation is not a paired speedup baseline for this standalone run.
Raw results and reproduction scripts are under target/language-contract-profile.

## Runtime construction optimization (completed, 2026-10-03)

TO01 owns the measured R4 bottleneck: reuse module-level dependency closure work,
share immutable checked native registrations/catalogs and cache the fixed
foundation/collection modules at host-thread lifetime. Keep fresh runtime heaps,
hosts, resources and generations, and all atomic installation/link checks.
No R6 verification change, test-route reduction, profile change, ABI migration
or unrelated queued work is activated. Commit the completed checkpoint with
`Roadmap-Step: TO01`.

- [x] Share dependency closure work without broadening binding requirements.
- [x] Reuse fixed checked modules without sharing runtime state.
- [x] Measure cold/warm construction and retain isolation/rejection coverage.
- [x] Accept workspace checks, feature consumers and CLI native tests.

Ledger: started from clean c4d18af0. The user authorized optimization after the
bounded diagnosis. Baseline construction is 931 ms warmed fresh-runtime median;
639 ms is per-binding dependency traversal. No build/test failure is carried.

TO01 implementation: module-level visited references/catalogs are reused when
calculating exact binding dependencies. NativeModule clones share immutable
registrations/storage and retained owned/full catalogs. Fixed foundation and
collection modules use thread-local OnceCell results; no unsafe Send/Sync or
runtime-state cache is added. Installations still stage and validate their own
registry, and linking still checks program facts and selected entries.

Three focused regressions pass: all foundation binding requirements match fresh
closure traversal; foreign requirements remain binding-specific and missing
dependencies reject atomically; shared registrations retain independent runtime
installations and roll back earlier entries in a failed batch. Structure checks
pass for 655 Rust files with zero violations/exceptions. Same-machine isolated
timing reports 1,079.989 -> 76.996 ms warmed medians (five samples) and
1,124.628 -> 481.447 ms first construction. Complete workspace regression passes
1,586 tests with zero failures and one existing ignored manual measurement.
The previously incomplete language-contract matrix passes in 327.88 seconds.
Workspace/all-target Clippy with -D warnings, structure, formatting and diff
checks pass. Independent artifact-only, source, native and source+native
consumers pass, with eight production dependency boundaries and the ABI build
graph checked. CLI jit-feature tests pass all five cases. Changed-document
file links pass. TO01 is accepted; no error or structural exception is carried.
Raw logs are under ignored target/runtime-construction. New host threads still
pay cold checked authoring; installation checks, source/SDK verification and the
serial language-contract matrix remain real costs. No whole-suite speedup is
claimed against the incomplete baseline, and no R6/test restructuring is activated.

## Test bottleneck diagnosis (completed, 2026-10-03)

The [performance record](performance-baseline.md#test-bottleneck-diagnosis-2026-10-03)
documents bounded measurements on clean 1b975a36. Runtime initialization accounts
for 81.1% of the 14 complete language-contract cases in a 90-second prefix;
five warmed fresh-runtime samples have a 931 ms construction median, including
639 ms rebuilding per-binding dependency catalogs. The single serial matrix
and feature-dependent Rust rebuilds amplify wall time; repeated SDK preparation
is secondary. Temporary instrumentation was restored; this is diagnosis only,
not full-suite acceptance or activation of R4/R6 or other queued migrations.

## Clippy maintenance (completed, 2026-10-03)

Scope: resolve the current workspace/all-target warnings on rustc 1.99.0 without
activating a queued architecture track. Replaced eight deprecated atomic
`fetch_update` calls with `try_update`, retaining relaxed ordering, checked
increments and exhaustion failures. Removed one unnecessary closure borrow in
the native concrete-result artifact limit check; validation remains unchanged.

Ledger: started from clean commit 478df5ff. Workspace/all-target Clippy with
`-D warnings`, formatting and structure (653 Rust files, zero violations or
exceptions) pass. An additional `cargo test --workspace` run was stopped after
the embedding language-contract integration case ran for about seven minutes
without completing; no assertion failure was reported, and full workspace
acceptance is not claimed. `cargo test -p kagari-common -p kagari-hir
-p kagari-runtime -p kagari-bytecode` passes 634 tests with zero failures or
ignored cases. Diff checks pass. Logs are under ignored
`target/clippy-maintenance-tests.log` and
`target/clippy-maintenance-focused-tests.log`. No imports, visibility, module
ownership, artifact versions or compatibility surface changed.

## Interface dispatch optimization (completed)

The [interface dispatch plan](interface-dispatch-optimization.md) implements R3
against the completed foundation API. ID01 shares immutable interface descriptors
and method selections; ID02 reuses receiver preparation while retaining per-call
generic arguments and constraint witnesses. Execution is authorized with one
commit per step. Both steps are accepted: interface descriptors and checked
selections are shared, closed receiver preparation and inherited views are reused,
and method-local arguments/witnesses remain call-specific. The plan records width
and sort evidence, cleanup/generation coverage, 1,583 passing workspace tests and
all independent feature routes. Fixed entry/construction and interpreter callback
costs remain. Other architecture review findings and queued migrations remain
separate work.

## Foundation API completion (completed)

The [foundation API completion plan](foundation-api-completion.md) implements
always-present basic types, collection traits/defaults and common methods, replacing
the optional bundled-algorithm installation switch. List/MutableList declare
algorithms as native-backed default trait methods; String owns built-in inherent
methods. No extension syntax, Deref or separate standard-library crate is added.

FA01-FA05 cover generic interface calls/default dispatch, foundation assembly, a
bounded list API, String methods and integration. All five phases are accepted.
Method-level generics work through interfaces using checked type/constraint argument passing and shared
entries, including script/native defaults and overrides; static calls retain
specialization. FA01 includes the required shared script-body compilation and
executable validation, not only native adapters. In-place collection operations
do not promise failure rollback: no atomic bulk-replacement requirement or
rollback-only buffering is introduced, and transaction-only sorting costs have
been removed. Final workspace coverage (with focused reruns for three superseded
assertions), Clippy, independent feature routes, structure/format checks and bounded
sorting measurements pass. The acceptance ledger records the remaining fixed
interface adaptation cost and current JIT boundary. Each phase has its own commit.
The completed reset below remains the starting baseline, not a full-library
restoration obligation. Contract/common renaming and the other queued migrations
remain separate.

## Native collections reset (completed)

The [native collections reset plan](native-provider-refactor.md) replaces the
previous ST/NR full-library restoration sequence. Execution is authorized in goal
mode with one commit per completed phase. Intermediate compilation failures are
allowed; temporary implementations to satisfy builds are explicitly rejected.
All four phases are accepted: obsolete libraries and artifacts are removed;
compiler-owned contracts/defaults, synchronous scoped native bindings and compact
traced storage are implemented; the bounded optional sorting/lazy-map and external
object proofs pass. Final workspace tests, Clippy, structure/format checks and
independent source/native feature consumers all pass. The plan records measured
costs and the disposition of obsolete consumers. Other queued plans below remain
separate work requiring activation.

The completed execution had four phases, in strict order:

1. Remove the old library crate, source/declaration catalogs, restored native
   packages/algorithms, standard-method selectors and tracked executable fixtures.
   Retain independently justified language/runtime infrastructure, not hidden
   library dependencies. Finish cleanup before replacement coding.
2. Implement complete compiler-owned language traits/types independently of native
   installation: equality/hash/ordering, operators/indexing, foundational
   List/MutableList, Map/MutableMap, Set/MutableSet, iteration, callable and existing
   formatting/range contracts. Declare ArrayList/HashMap/HashSet as canonical
   defaults and provide their minimal Rust runtime implementations through ordinary
   native bindings. Basic operations and array literals remain available without
   optional modules; hash bounds belong to the concrete types.
   Default backing is Rust std::collections::HashMap/HashSet; indexmap belongs to
   optional standard-library LinkedHashMap/LinkedHashSet.
3. Define Kagari modules through ModuleBuilder with scoped implement/trait_impl
   blocks. Ordinary bind checks Rust conversion views against existing Kagari
   signatures; bind_with supplies explicit NativeBinding codecs where needed.
   Retire macro-derived declarations.
   Replace mandatory continuations with efficient synchronous functions and
   callbacks. Prepare targets once; register native object storage/GC hooks without
   adding concrete type variants to generic layers. Use compact primitive buffers
   and scoped bulk access. Lazy cursors retain persistent state; genuine
   asynchronous suspension alone needs resumable execution machinery.
4. Prove one optional algorithm module over the language's ArrayList, including
   contiguous i32 storage, script comparators, lazy map, tooling declarations,
   source-free execution and measured costs. The existing external consumer also
   proves non-sequence native storage registration. Extended Map/Set algorithms
   and full-library restoration remain deferred; basic default hash storage belongs
   to phase 2. Other containers and extension algorithms belong to optional modules.

The plan owns exact scope, acceptance and the only active checklist/ledger.
Budget schedules and permission matrices do not constrain its native ABI or
acceptance. No routine version bumps, repeated complete artifact regeneration or
inherited ST06/NR05 matrix is required. Full repository checks belong to final
integration; focused checks serve intermediate implementation.

The [standard-library integration document](stdlib-hir-refactor.md) and earlier
ST/NR commits are historical evidence. Their complete restoration obligations and
per-step execution model are superseded, not carried into this reset. Retained
language and safety behavior still requires meaningful coverage. Obsolete
consumer failures are retired explicitly; failures in retained consumers must
be resolved by final acceptance.

## Foundation trait ownership correction (accepted)

Compiler-owned core now includes Into, TryFrom, TryInto, FromStr, FromIterator,
Sum and Product, completing the original 38-trait set.
No Try, FromResidual or other new trait is added. The [existing plan's boundary
correction](native-provider-refactor.md#approved-foundation-boundary-correction)
owns the finite follow-up; concrete algorithms and additional containers retain
their runtime/library owners. Execution, source-free artifact and feature checks
pass; the plan ledger records the workspace sweep and focused regression fixes
without reopening the completed reset checkpoints.

## Contract and common responsibility cleanup (queued)

The approved name is `kagari-contract`, replacing `kagari-abi`, with domain type
names such as `Type`, `FunctionDecl` and `TraitDecl` instead of `Abi` affixes.
The [architecture decision](architecture.md#contract-and-common-responsibility-cleanup)
records the naming map and the joint ABI/common ownership review. This is a
documentation checkpoint; code migration is not active and does not reopen the
completed native collection reset.

The finite implementation scope is to map current consumers and dependencies,
separate portable contracts, language foundations, source-independent verification,
native registration and source/tooling responsibilities, then migrate the crate,
types and affected consumers/checks together. Review common's source utilities,
identities, numeric semantics and host schemas in the same change sequence.
Resolve duplicate declaration ownership rather than adding aliases. The final
module map determines whether common remains; no extra crate count is prescribed.

Preserve existing semantics and source-free execution/validation. Do not add
library features, execution-policy redesign, compatibility workflows or an FFI
implementation to this scope. `kagari-ffi` is recorded only as the future external
C adapter boundary. Activation, phase breakdown and implementation acceptance
checks remain pending; this queue entry is the progress record until activation.

## Installation access and cancellation (complete)

The [execution policy plan](execution-policy-refactor.md) completed EP01-EP03 on
2026-10-02. Duplicate authorization and execution charging are removed; installed
interfaces, cooperative root cancellation, runtime call-depth protection and
correctness checks remain. No max_work or general heap quota replaces charging.
The plan records the passing workspace, standalone feature/JIT checks and baseline.
ABI/common ownership cleanup remains queued separately; capability and cost fields
are deleted here rather than moved to the future contract crate.

## Rust value and opaque interoperability queued

The [Rust interoperability plan](rust-interop-design.md) defines ordinary typed
value conversion, an optional schema-backed Serde adapter and retained opaque
objects. Module registration owns type names; parameters, results and properties
share recursive binding rules. Opaque wrappers are cloneable without cloning their
payloads, and modifying methods use host-managed interior mutation. Rust borrow
exposure and automatic exclusive receivers remain separate future work.
RI00-RI05 are queued after native provider unification, with proposed execution
after execution-policy simplification. This synchronous binding work does not
depend on async or change its prerequisites; activation and exact scheduling remain
pending. No current ST/NR scope is expanded.

## Host API unification queued

The [host API refactor plan](host-api-refactor.md) unifies preparation, loading,
calls and reload around Engine, Program, Runtime and a stable Script installation.
Normal calls infer Rust types and use an explicit outer tuple as the argument list;
each tuple element is one script value, with no implicit spreading of tuple returns.
HA00-HA05 follow native provider, execution-policy and Rust interop acceptance.
Latest-version function entries with root-pinned execution are a proposal to confirm
at activation. The synchronous facade does not require async implementation or
change the existing async prerequisites; later integration shares its type and
version contracts. This is planning only and does not expand current migrations.

The package and update proposals below refine HA's previously open identity and
reload-compatibility gates. Freeze their contracts before HA's affected phases;
UP owns lower-level update validation/cutover and HA owns the public facade. This
is not a requirement that both whole tracks finish before either can begin.

## Package and dependency design queued

The [package design](package-design.md) proposes Cargo-style dependency declarations,
stable logical identities, exact resolved graphs and source-to-module mapping.
PK00-PK04 distinguish packages from executable Programs and runtime installations.
The first-delivery source kinds, single-selection policy and manifest defaults are
review choices. Freeze package identity before HA's package input and UP's compatibility
implementation. Activation follows the active ST/NR work; exact placement alongside
EP/RI/HA remains to be agreed without adding scope to those active predecessors.

## Compatible hot reload and state replacement queued

The [update model](update-model-design.md) separates compatible code publication
from explicit player-state export, fresh-environment restoration and host cutover.
Compatible updates preserve existing contracts and may add new concrete types with
trait implementations; adding trait impls to old types is excluded. State replacement
permits code restructuring but carries data, not old tasks or runtime object identities.
UP00-UP05 own compatibility and cutover; RI supplies conversion/root foundations,
PK supplies identity and dependency facts, and HA supplies embedding entrypoints.
Synchronous acceptance does not require async. Async integration must later cover
quiescence, cancellation and late completions using the same update boundaries.

## Async script and native execution design

The [async execution proposal](async-execution-design.md) describes typed native
operations awaited by async scripts, with host-driven single-threaded execution.
It is a design-only follow-up after native provider unification and execution-policy
simplification, not an active implementation phase. Task semantics, owned execution
lifetimes and completion
contracts require review before activation. Ordinary callback-bearing natives
remain distinct from external async waits; no async work is added to ST or NR
acceptance, and multi-threaded script execution remains out of scope.

The companion [host task scope design](host-task-scope-design.md) covers synchronous
handlers launching scope-owned async work, generic Native registration and
Actor-mailbox dispatch through a bounded drive API. It refines the async proposal's
task lifetime and scheduling contract without adding Actor or Tokio policy to core
execution or creating another active implementation track.

## Never type (complete)

Scope: introduce the uninhabited type `!` in source, checked types, executable
contracts and artifacts. A diverging expression coerces at an expression boundary
to any expected type; this does not make generic containers covariant. Calls whose
declared result is `!` have no normal continuation. Return/break/continue remain
statements. Loops retain ordinary execution budgets. `Infallible` remains a distinct
empty enum; no compatibility alias or runtime Never value is introduced.

- [x] N01: syntax, canonical types, declaration metadata and type inference.
- [x] N02: lowering, executable validation, runtime boundaries and artifact versions.
- [x] N03: source/artifact/native-fallback tests, negative contracts and final checks.

Acceptance: `panic -> !`, user and generic diverging calls, divergent branches and
loops, closures, `Result<T, !>`, empty matches, invariant generic arguments,
rejected normal returns, preserved effects/traps/budgets and rejected invalid
executable values. Run structure, format, workspace Clippy/tests and diff checks.
Checkpoint trailers: `Roadmap-Step: N01`, `N02`, `N03` as applicable.

Ledger: started from clean commit 71fdea8e. Existing control-flow completion is
syntax-based and has no call result facts; Never must extend that analysis rather
than authorize code generation through Unknown recovery types. No existing debt
or carried build errors at entry.

N01/N02: added Never as an explicit semantic/physical type and declaration result;
completion consumes checked type facts and keeps address evaluation separate from
the stored type. Coercion stays at expression boundaries. Diverging closure results
use a deferred Never fallback after other constraints; non-completing return
operands contribute no returning value. Lowering terminates Never evaluations and
callable and native collection interface adapters; both executable verifiers
reject normal Never returns.
KBC v104, runtime ABI v103 and KMIR v2 reject old products. Regenerated the existing
SDK feature fixture using its documented source/SDK recipe.

N03: six SDK integration tests cover source/artifact/JIT-fallback execution,
generic and closure inference, trait/callable/native collection interfaces,
uninhabited containers, short-circuiting, normal-return and invariant-container
rejections, effect order, trap cleanup and loop budgets. Two compiler tests cover
Never termination and forged Never-typed returns in MIR and bytecode. The existing
terminating-assignment regression and regenerated feature fixture tests pass.

Final acceptance: `cargo test --workspace` passes 1,442 tests, including standard
API documentation examples and doctests; `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo fmt --all -- --check`, `uv run --locked
scripts/check_structure.py` and `git diff --check` pass. Structure review covers
516 Rust files with no violations or exceptions. Cargo checks ran serially after
an earlier overlapping rebuild caused a transient SDK doctest rlib lookup error;
the final complete run resolves it. No carried build or test errors remain.
Logs are under ignored `target/never-workspace-final.log`, `target/never-clippy.log`
and `target/never-structure-final.log`. Native backend coverage remains unchanged;
unsupported operations use the existing verified fallback.

## Completed architecture track

The [MIR and crate architecture refactor](mir-architecture-refactor.md) is complete
through A00–A05. It records the mandatory clean A00 baseline, the thirteen-crate
migration and final acceptance. The [architecture](architecture.md) and current
specifications describe the resulting implementation; the plan ledger preserves
phase decisions and validation evidence.

Final acceptance passes: 1,434 workspace tests, workspace Clippy/format checks,
514 Rust files with no structure violations or exceptions, eight production
dependency audits, four isolated SDK feature consumers, and CLI native-feature
tests. The [performance baseline](performance-baseline.md#mir-architecture-baseline-2026-09-28)
records O1 measurements and their limits. There are no carried architecture build
or test errors. Broader Cranelift coverage remains the next separately scoped native
track; LLVM is deferred until after that work.

[Foundation refactor](foundation-refactor.md) records the completed R01–R18 track.
Its semantic contracts, as amended by later specifications, continue to govern
behavior. Its `Roadmap-Step: Rxx` trailers and the
[performance baseline](performance-baseline.md) are historical evidence, not an
additional active migration queue.

The former M1–M11 milestone queue is historical and has been removed from this document. Git history retains its original scope and commits; those milestones do not prescribe current APIs, compatibility branches, artifact formats, or acceptance criteria.

After the architecture track, native backend expansion prioritizes Cranelift JIT;
LLVM is a late independent track. Complete LSP/editor integration, a full
incremental dependency database, async and cross-thread execution policy,
incremental or generational GC, complete event replay, persistent state migration
and further standard-library coverage remain separately scoped work. These tracks
reuse the existing semantic contracts.

Completed language extension: ordinary associated types, equality bindings,
projection bounds and qualified projections, preserving the existing static and
dynamic interface model. The [trait contract](spec/traits.md#ordinary-associated-types),
[runnable example](../examples/syntax/associated-types.kgr) and associated-type
integration tests define this checkpoint. Subsequent trait extensions are
recorded below; advanced coherence/solver features remain future work.

Completed language extension: generic implementation interface tables, keyed by
impl declaration and concrete arguments, with shared instantiation limits and
cross-module method reachability. See the [trait contract](spec/traits.md#generic-implementation-interface-instances)
and [runnable example](../examples/syntax/generic-interfaces.kgr). Artifact format
44 and runtime ABI v44 reject prior formats without migration.

Completed language extension: host associated-output declarations and dynamic
interfaces, using offline contracts and verified IR forwarding functions through
the existing host boundary. See the [trait contract](spec/traits.md#host-associated-outputs-and-interfaces)
and [embedding example](../crates/kagari-embed/examples/host_interfaces.rs).
Integration tests cover source/artifact execution, JIT-enabled execution,
cross-module generic inputs, GC, reentry, traps and retained reload versions.
Artifact format 45, runtime ABI v45 and KHI v10 reject previous products.

Completed language extension: bounded, acyclic trait inheritance, transitive
static bounds, inherited associated projections and dynamic interface upcasting.
Diamond paths deduplicate the same applied declaration; unrelated same-named
methods are ambiguous. Source, artifact and JIT fallback tests cover cross-module
generic parents, invalid graphs, GC and retained reload versions. See the
[trait contract](spec/traits.md#trait-inheritance-and-upcasting) and
[runnable example](../examples/syntax/trait-inheritance.kgr). Artifact format 46
and runtime ABI v46 reject previous products; KHI remains v10.

Completed language extension: checked default method bodies, explicit override
precedence and fallback for script impls, including generic impls, method-local
generics, associated outputs, closures and imported private helpers. Defaults
are specialized in the impl's module while retaining the trait's checked name
resolution and original debug source. See the [trait contract](spec/traits.md#default-methods)
and [runnable example](../examples/syntax/default-methods.kgr). Artifact format 47
and runtime ABI v47 reject previous products; KHI remains v10. Host mappings
continue to explicitly provide every declared method.

Completed language extension: scalar associated constants, required definitions,
defaults and overrides, inherited access and qualified static paths. Source and
artifact validation reject constant-bearing dynamic interfaces, including
subtraits. Artifact format 48 and runtime ABI v48 reject previous products;
KHI remains v10. Native host tables cannot supply constants; use script impls.

Completed language extension: type-parameterized generic associated types,
declaration-owned constructor binders, input and output bounds, inherited
projections, generic impls and defaults. Source, encoded artifacts and JIT-enabled
execution cover static specialization, imported contracts and malformed unused
metadata. See the [trait contract](spec/traits.md#generic-associated-types) and
[runnable example](../examples/syntax/generic-associated-types.kgr). Artifact
format 49 and runtime ABI v49 reject previous products; KHI remains v10.

Traits declaring associated constants or GAT, including every descendant,
are static-only. Native host tables cannot supply those members; script impls
on host types can. Lifetimes, script-level `dyn`, higher-kinded type parameters,
associated type defaults, specialization, negative impls, auto traits and
advanced coherence/solver behavior are outside this sequence.

Completed language extension: built-in Option/Result constructors, namespace and
alias imports, nested and alternative patterns, postfix `?`, explicit Option to
Result conversion, and checked script callbacks for map/map_err/and_then.
Source, encoded artifacts and JIT fallback share ordinary frame control flow,
including GC and iteration cleanup. See [builtins](spec/builtins.md#option-and-result)
and [result-option.kgr](../examples/syntax/result-option.kgr). Artifact format 50
and runtime ABI v50 reject previous products; KHI remains v10. General built-in
propagation traits remain deferred; error origins/stacks are completed in E01–E03 below.

Completed standard-protocol checkpoint: declaration-owned PartialEq/Eq/Hash and
Debug/Display, ordinary generic and associated bounds, sealed intrinsic equality
and hashing, explicit nominal formatting impls, structural and identity keys,
and GC tracing of keys. See [contracts](spec/builtins.md) and
[standard-traits.kgr](../examples/syntax/standard-traits.kgr). Artifact format 51,
runtime ABI v51 and KHI v11 reject old products. Protocol interface boxing
and generalized propagation remain separate later checkpoints. Error origins
and stacks are completed in E01–E03; Iterator/Iterable are completed in B08 below.

Completed equality checkpoint: object identity operators `===`/`!==`, checked
from syntax through IR and VM, with artifact format 52 and runtime ABI v52.
Source/artifact/JIT-fallback tests cover aliases and separate allocations; runtime
tests reject foreign, stale and mistagged handles.

Completed custom equality and hashing checkpoint: explicit Struct and enum
PartialEq/Eq/Hash implementations, generic bounds, recursive composite helpers,
and guarded Map/Set callbacks. Native defaults retain their fast path. Type-owned
implementations keep dependency and caller semantics consistent. Tests cover
cross-variant enum equality, nested keys, collisions, GC, callback traps, reentry,
budget cleanup, portable metadata and source/artifact/JIT fallback execution.
See the [contract](spec/value-semantics.md#equality-and-hashing) and
[example](../examples/syntax/standard-traits.kgr). Artifact format 53 and runtime
ABI v53 reject previous products; KHI remains v11. Key stability and equivalence
laws are user obligations; there is no automatic freezing or reindexing.


## Standard operator protocols

The B01–B06 extension sequence is complete. Each checkpoint updates contracts,
examples and relevant tests and is committed separately using Conventional Commits.
Clone, writable indexing and compound-assignment overrides are separate designs.

- [x] B01: declaration-owned standard protocol inputs, associated outputs and parent metadata shared with portable contracts.
- [x] B02: Ordering, PartialOrd and Ord, including checked comparison dispatch.
- [x] B03: Add/Sub/Mul/Div/Rem with an explicit RHS type and associated Output.
- [x] B04: Neg/Not with associated Output.
- [x] B05: read-only Index; returned objects retain shared reference semantics.
- [x] B06: source/artifact/backend conformance, examples, documentation and workspace checks.

Builtin operations retain direct instructions. Custom implementations use ordinary
static linked calls. Operands evaluate once from left to right. These protocols
remain static-only in this sequence. Short-circuit and identity operators cannot
be overridden. Existing assignment and mutation guarantees are unchanged.

The sequence uses artifact format 57 and runtime ABI v57; earlier products are
rejected without migration. KHI remains v11. The operator conformance suite checks
source, encoded artifacts and JIT-enabled fallback with collection at allocation
safepoints, cross-module generic implementations, single evaluation and target
identity, malformed contracts, unordered floats and preserved builtin fast paths.

B06 validation: 1,116 workspace tests passed (including 17 operator integration
cases); `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
-D warnings` and `git diff --check` passed. Integration cases also cover applied
RHS overload selection and resource cleanup after operator traps/budget exhaustion.

## Conversion and iteration protocols

- [x] B07: explicit From/Into and TryFrom/TryInto, static conversion calls, derived reverse bounds and associated errors.
- [x] B08: Iterator/Iterable contracts, custom for loops and native collection integration.
- [x] B09: conformance, examples, resource cleanup and workspace validation.

Each checkpoint uses a Conventional Commit. Conversion calls are explicit; reverse
protocols are derived and cannot be implemented independently. Iteration preserves
native structural-mutation guards; custom iterators own their consistency rules.
General propagation, Clone and writable indexing remain separate. Error origins
are completed in E01–E03 below.
Result propagation now also uses From for error conversion; see the propagation
checkpoint below for the current contract.

B08 uses artifact format 59 and runtime ABI v59. Custom iterators and native
iterators share protocol-based for lowering. Native iterators retain source guards,
GC roots and code versions; loop/session cleanup and structural revision checks
cover early exit and resumed iterators. See the authoritative iteration contract in
[the builtins specification](spec/builtins.md#iteration-protocols).

B09 validation: 1,132 workspace tests passed, including 6 conversion and 10
iteration integration cases. Source, artifact roundtrips and JIT fallback cover
single evaluation, qualified conversions, derived bounds, cross-module generic
implementations, native/custom iteration and associated output checking. Invalid
iterator instructions are rejected before execution. Cleanup tests cover nested
loops, return, trap, cancellation, budgets, rooted iterators across GC, foreign/stale
handles and structure changes between calls. Formatting, workspace clippy with
warnings denied, and `git diff --check` passed. The former generic Iterator<T>
proposal now references the implemented associated-Item contract.


## Error origins and diagnostic stacks

- [x] E01: bounded source-aware stack snapshots for runtime failures, including native JIT program points and portable source locations.
- [x] E02: Result Err origin metadata, preservation through propagation/combinators, host reports and CLI rendering.
- [x] E03: source/artifact/backend, lifecycle and diagnostic conformance; documentation and workspace checks.

Errors remain Result values. New Err captures its origin; propagation preserves it.
Reconstruction creates a new origin. None does not carry an error stack. Error
traits, cause chains and generalized propagation remain subsequent features.

E02 uses artifact format/runtime ABI 61 and helper ABI 6. The authoritative
[error-reporting contract](spec/error-reporting.md) covers origins, metadata
propagation, host inspection and CLI presentation.

E03 validation: 1,148 workspace tests passed, including 13 error-reporting
integration cases. Coverage includes imported frames, UTF-8/CRLF locations,
minimal artifacts, source edits after compilation, native overflow positions,
propagation and reconstruction, equality/hash independence, bounded recursion,
GC/reload, host reentry and cancellation/budget cleanup. Malformed mapped-error
contracts and registers are rejected before execution. CLI source/artifact
reports agree; the optional JIT CLI test also passed. Formatting, workspace
clippy with warnings denied and `git diff --check` passed.


## Standard library declaration sources

Source comments and API documentation are written in English.

- [x] S01: declaration parsing, documentation and source-location foundation.
- [x] S02: source-owned standard function signatures and method bindings.
- [x] S03: standard enum/type declarations and the 21 standard trait contracts.
- [x] S04: shared semantic queries for navigation, documentation and signatures.
- [x] S05: executable documentation, binding validation, removal of duplicate definitions and workspace acceptance.

This sequence migrates all 71 existing public standard functions and 54 method
views without expanding their runtime semantics. Host API declaration generation
and the LSP transport remain subsequent checkpoints. Native layout, GC, mutation,
resource and bytecode-validation contracts remain engine responsibilities.

S02 migrated 71 function signatures and 54 method views into English declaration
sources with executable examples. It removed duplicated intrinsic call typing and
connected native for_each to ordinary script closure frames and iterator cleanup.
Validation includes 327 HIR tests, 146 IR tests and source/artifact documentation
execution. Rustdoc-style API documentation is required for subsequent declarations.

S03 replaced handwritten standard enum, type-constructor and 21 trait signature
builders with source-derived metadata. S04 connected native function and method
navigation, type/variant definitions, trait members, Markdown, instantiated native
signatures and incomplete native-member candidates to the bundled source catalog.

S05 provides 100 executable English examples across 71 functions, eight native
types/enums and 21 traits. Documentation follows Rust's summary, behavior,
applicable Panics and Examples structure while describing Kagari semantics.
Native enum discriminants and payload types are validated as runtime ABI bindings;
public signatures must instantiate without unknown/error types. The standard API
index is stdlib/README.md (retired predecessor file). Full LSP transport, lexical trait
completion, scalar reference pages and host declaration generation remain future
work. This batch does not implement those separate features.

Acceptance: 1,159 workspace tests passed. All 100 documentation examples passed
source and artifact execution; formatting, workspace clippy with warnings denied
and `git diff --check` passed.

Declaration style follow-up (2026-09-28): 35 standard-library function and method
declarations now omit redundant unit return annotations. Omitted returns already
resolve to unit; callback function types retain their required explicit returns.
The declaration specification records this convention. All three tests in
`cargo test -p kagari-embed --test standard_declarations` pass, including the
source/artifact documentation examples. Structure, formatting and diff checks pass.

## Rust-style outer attributes

The attribute spelling is `#[path(...)]`, including argument-free `#[meta]`.
The legacy `@...` spelling is removed without a compatibility parser. Attribute
arguments, semantic validation and native bindings retain their existing behavior.
Inner `#![...]` attributes and a macro system are not introduced. The grammar,
standard declarations, examples and tests use the same outer-attribute syntax.

Validation: 1,161 workspace tests passed, including the 100 standard API
examples. Formatting, workspace clippy with warnings denied and `git diff --check`
passed. Recovery tests cover malformed delimiters, rejected legacy/inner attributes,
lossless nested metadata and following declaration recovery.

## String interpolation and joining

- [x] Lossless, bounded `f"..."` parsing with expression holes, escaped braces,
  ordinary text escapes and nested interpolation.
- [x] Canonical Display/Debug selection, left-to-right single evaluation and
  ordinary propagation/trap cleanup through existing call frames.
- [x] `ArrayList<String>.join(separator)` and `std::array::ArrayList::join`, using checked byte-length
  accumulation and one result-buffer reservation; concat remains available.
- [x] English API docs, runnable example and EBNF coverage inventories.

Artifact format and runtime ABI are v62. The helper ABI remains v6. Old artifacts
are rejected; no compatibility decoder is introduced. Width/precision formatting,
String `+`, StringBuilder and compile-time interpolation remain separate work.

Acceptance: 1,173 workspace tests passed, including source/artifact/JIT-path
interpolation tests, 101 standard API documentation examples, Unicode/source-query
rebasing, parser limits and native argument validation. Formatting, workspace
clippy with warnings denied and `git diff --check` passed.

## Collection access and construction

The authoritative [collection contract](spec/collection-access.md) defines
compiler-owned interfaces and canonical ArrayList/HashMap/HashSet defaults. The
CI01 results below describe the predecessor LinkedHashMap/LinkedHashSet storage,
not acceptance of the unimplemented reset. `[T]` means read-only List; array literals
create ArrayList. Constructors and collection destinations name concrete storage.

The earlier C00-C04 checkpoints established access checking, host ABI access flags,
artifact validation, shallow factories and source queries (KBC v63-v64, KHI v12).
Their public paired-native-type surface is superseded by CI01 below. Host storage
access flags remain useful independently of script interface dispatch.

- [x] CI01: declare six storage-independent interfaces and native impl witnesses;
  support generic and dynamic dispatch, inherited access, native bridge validation,
  readonly views and script-defined implementations; migrate constructors,
  collection destinations, documentation and examples; advance KBC/runtime ABI v84.

Native direct calls retain intrinsic paths. Dynamic interface values retain their
underlying object and version through ordinary GC-managed interface wrappers.
Map/Set interfaces do not require Eq/Hash or define iteration order. Initial linked
hash implementations require Eq/Hash and preserve current insertion order.
Other concrete containers, variance, deep freezing and general clone remain
separate work. Existing failure/identity contracts still apply.

CI01 validation: all 1,275 workspace tests passed (1,274 ordinary tests plus the
standard documentation test executing 337 API examples). Source/artifact/JIT
fallback coverage includes native and script collection implementations, writable
views, alias identity/hash, custom keys and access rejection. Workspace formatting,
clippy with warnings denied, and `git diff --check` passed. The public SDK and
executable collection examples use the new interface/storage split.

C01a validation: 1,176 workspace tests passed, including 101 executable standard
API documentation examples. Workspace clippy with warnings denied, formatting
and `git diff --check` passed.

C01-C04 validation: 1,188 workspace tests passed, including 114 executable
standard-library documentation blocks. Collection integration tests exercise
source/serialized-artifact/JIT-enabled fallback, live views and shallow snapshots,
custom Eq/Hash, input iteration guards, GC during host reentry, host binding access
mismatches, malformed executable access contracts and negative writes through
methods, free functions, indexing, reflection, generics, closures and branch joins.
Constructor navigation and read-only member completion have query coverage.
`cargo fmt --all -- --check`, workspace/all-targets clippy with warnings denied,
`cargo test --workspace --no-fail-fast` and `git diff --check` passed.

## Standard methods and lazy collection pipelines

This sequence replaces the previous method aliases and native-only iteration helpers.
Each checkpoint includes specifications, examples, relevant tests and a Conventional
Commit. No compatibility aliases are retained.

- [x] I01: source-owned inherent methods and associated functions in generic impl
  blocks; remove method attributes and migrate native API calls and queries.
- [x] I02: unify Iterable/Iterator and iter, including derived iterator identity,
  for-loop conversion, associated outputs and user-defined protocols.
- [x] I03: FromIterator and target-directed collect for all six collection types
  and user-defined collections; fresh shallow construction and checked key insertion.
- [x] I04: lazy map, filter, filter_map, take, skip, enumerate, zip and chain.
- [x] I05: find, any, all, count, fold, for_each, partition and whole-input group_by.
- [x] I06: shared iterator progress, short-circuit continuation, guard lifetimes,
  callback failures, budgets and GC retention across adapter chains.
- [x] I07: executable English API documentation, examples, source/artifact/backend
  conformance and final workspace formatting, clippy and test validation.

Kagari uses iter without ownership transfer; callbacks receive ordinary values.
group_by is a Kagari extension returning LinkedHashMap<K, ArrayList<T>>. flat_map,
flatten, sum/product and Result/Option collection lifting are covered by J01-J06 below.

I01 validation: HIR and embedding tests passed, including executable standard API
examples, native declaration navigation, removed-export rejection, receiver access
and concrete List<String> method checks. Workspace check and git diff --check passed.

I02 replaces IntoIterator/into_iter with the canonical Iterable/iter protocol and
associated Iter type. Iterable is no longer resolved as a sealed source constraint.
Collection calls create independent progress; iterator calls preserve aliases and
position. Existing native-helper predicates are internal and will disappear with
the old helpers in I05. KBC/runtime ABI v65 rejects earlier artifacts.
Iteration, conversion, declaration examples and embedding tests passed; the HIR
recovery assertion now recognizes ordinary trait-bound diagnostics. git diff --check
passed.

I03 adds generic standard method contracts, FromIterator<T> and Iterator::collect<C>.
All six collection targets share construction with existing from(array) factories.
User destinations use statically specialized ordinary frames. Method bounds substitute
Self and associated outputs before checking. Native defaults appear in portable
implementation contracts without fictitious script bodies. KBC/runtime ABI v66 rejects
earlier artifacts. HIR, IR and embedding tests passed, including invalid bounds,
fresh destination access, user destinations, duplicate policies and API documentation.

I04 adds opaque Iter adapters driven by ordinary VM closure frames. Construction
does not invoke callbacks; aliases share progress. Generic callbacks and secondary
Iterable inputs retain associated item types. HIR, IR and embedding tests passed,
including all eight adapters through source, serialized artifacts and JIT fallback
with collection at every allocation. KBC/runtime ABI v67 rejects older encodings.

I05 adds terminal defaults, target-directed partition and whole-input group_by.
Old native-only iteration functions, callback implementations and operation
predicates were deleted; source examples use iterator methods. KBC/runtime ABI
v68 rejects older intrinsic encodings. HIR, IR, runtime, VM and embedding tests
passed after updating the renamed native diagnostic expectation. Tests include
short-circuit continuation, empty inputs, custom collection targets, colliding
custom keys, negative callback/bound checks and inference-error preservation.

I06 replaces native iterator snapshots with on-demand indexed reads and UTF-8 scalar
progress. Source shape is checked at construction and yielded payloads are checked
before progress commits. Guard acquisition/cleanup use explicit work lists, share
duplicate dependencies and retain roots across ordinary callback frames. Runtime,
VM and embedding tests passed, with additional acceptance for 1,500 adapter layers,
constant iterator allocation size, live slot replacement, trap/budget cleanup, and
rooted pipelines resumed through host reentry after GC between root sessions.
KBC/runtime ABI v69 rejects artifacts with earlier iteration semantics.

I07 completes English method documentation and adds collection-pipelines.kgr.
The bundled API now contains 127 executable documentation blocks. Native qualified
from_iter paths and trait-default navigation/signature queries have acceptance
coverage. Generated adapter steps retain their originating call-site identity and
debug location; removed native-only iterable facts no longer form a parallel
semantic model. Negative tests reject malformed closure iterator contracts and
invalid usize state, and explicit user overrides retain ordinary static dispatch.

Final validation: 1,206 workspace tests passed, including source/serialized-artifact
execution, JIT-enabled fallback, all standalone examples and API documentation.
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --no-fail-fast` and `git diff --check` passed.

The native iterator type is now Iter<T>, replacing Cursor<T> without an alias.
Its declaration, type identities, ABI nodes, IR/bytecode instructions, runtime
operations and examples use the same name. Iterator and Iterable::Iter keep their
protocol meanings; an implementation can select `type Iter = Iter<i32>` and return
Self::Iter. Artifact format/runtime ABI v70 reject earlier products.

Rename validation: 1,207 workspace tests passed, including independent resolution
of the concrete Iter type and Iterable::Iter, removed-name rejection, checked
signature display, source/artifact/JIT iteration and executable API examples.
Formatting, workspace/all-targets clippy with warnings denied and git diff --check
also passed.

The native iterator now has an explicit `impl<T> Iterator for Iter<T>` declaration
in `stdlib/iter.kgr`. The build validates its native `next` binding and generates
implementation metadata, associated type mappings and member source identities.
Type checking reads the declared `Item` mapping; tooling can discover the concrete
implementation and complete its methods alongside inherited Iterator defaults.
The runtime stepping contract and artifact ABI are unchanged.

Declaration validation: 1,208 workspace tests passed, including implementation
member locations, incomplete-member completion, source/artifact/JIT iterator
execution and all executable standard-library documentation. Formatting,
workspace/all-targets clippy with warnings denied and `git diff --check` passed.

## Iterator extensions

- [x] J01: conditional adapters, inspection/fusion, lookup, reduction and comparator extrema.
- [x] J02: declaration-level where bounds, ordered extrema and key-based extrema.
- [x] J03: lazy flat_map/flatten with dynamically retained inner iterators.
- [x] J04: Sum/Product protocols and target-directed numeric/user-defined aggregation.
- [x] J05: Result/Option FromIterator lifting with short-circuiting and preserved error origins.
- [x] J06: explicit native collection Iterable/FromIterator declarations, examples and final validation.

Fallible collection first buffers successful items, then invokes the destination's
FromIterator only after the source ends successfully. A failure returns immediately
without invoking destination construction; completed source effects remain.
Generic Try/FromResidual, try_fold/try_for_each, peekable, double-ended iteration
and exact-length protocols remain separate follow-up work.

J01 validation: HIR tests, lazy iterator source/artifact/JIT tests and all bundled
API documentation examples passed. KBC/runtime ABI v71 rejects earlier contracts.

J02 adds source-owned method where bounds and qualified associated projections.
Ordered extrema and exactly-once key evaluation pass source/artifact/JIT tests;
invalid Ord bounds are rejected. All executable API examples passed.

J03 validates lazy one-level flattening, custom Iterable outputs, empty inner
iterables, shared progress and dynamic guard cleanup with GC at every allocation.
Canonical native defaults no longer duplicate conditional method signatures in
every implementation ABI. IR verification tests and iterator route tests passed.

J04 provides target-directed Sum/Product with numeric identity values and ordinary
user implementations. Tests cover generic callers, custom targets, empty inputs,
float/double identities and usize aggregation across source/artifact/JIT routes.
IR and bytecode now represent native f64 constants for the empty product identity.

J05 validates Result/Option lifting into native and user destinations, nested
wrappers, generic destination bounds, empty inputs and unconsumed source tails.
Original Err stack metadata survives collection and GC. Iterator, error-trace
and executable documentation tests passed across source/artifact/JIT routes.

J06 moves native collection and fallible wrapper implementation facts into their
SDK declarations, including generic key and destination constraints. API queries
retain concrete member locations. Associated iterator bounds preserve their
originating Item equalities, and tolerant analysis retains collection shape even
when element types are erroneous. Narrow integer aggregates check the destination
range before updating the accumulator.

Final validation: 1,217 workspace tests passed, including 38 standalone examples
and 158 executable API documentation blocks. Tests cover lazy nested state across
GC and host reentry, native guard cleanup, short-circuiting, original Err stacks,
custom destinations and numeric overflow. Formatting, workspace/all-targets clippy
with warnings denied and `git diff --check` passed. KBC/runtime ABI v76 rejects
earlier products.

## Body inference and numeric literals

- [x] T01: body-local inference variables, structural unification, occurs checks and cancellation.
- [x] T02: infer local bindings and empty containers from subsequent uses.
- [x] T03: solve calls, constructors, branches and closures independently of source order.
- [x] T04: propagate constraints through iterator chains and associated types.
- [x] T05: numeric suffixes, contextual numeric inference, checked ranges and f64 fallback.
- [x] T06: local type placeholders and explicit function/method type arguments.
- [x] T07: diagnostics, source/artifact/backend acceptance, examples and final validation.

Inference is bounded by a function body; declaration signatures remain explicit.
Inference variables are distinct from unknown/error recovery facts. Evaluation
order and collection write permissions remain unchanged. Numeric suffixes cover
existing primitive types only; out-of-range literals are compile errors.

The executable [type inference example](../examples/syntax/type-inference.kgr)
combines later-use inference, phantom generic parameters, local `_` holes,
explicit function/method type arguments and checked numeric literals. This remains
body-local inference, with explicit declaration signatures and bounded solving;
it does not introduce whole-program inference or implicit numeric conversions.

Final validation: 1,232 workspace tests passed, including 39 standalone examples
and 158 executable API documentation blocks. Acceptance covers source compilation,
encoded artifacts and JIT-enabled execution with interpreter fallback. Negative
tests cover explicit type conflicts, unresolved holes, literal overflow and
malformed integer constants; tolerant analysis retains independently known types.
Formatting, workspace/all-targets clippy with warnings denied, and
`git diff --check` passed. KBC/runtime ABI v77 rejects earlier products and carries
u64/usize values without signed truncation.

## Result propagation through From

- [x] Select the canonical `F: From<E>` contract when `Result<T, E>?` returns through `Result<U, F>`.
- [x] Lower conversion to the ordinary static call path only on Err; retain original failure metadata.
- [x] Preserve identity conversion, Option behavior, generic bounds, closure inference and source-module linking.
- [x] Document direct conversion, trap cleanup and the absence of conversion-chain searches or TryFrom fallback.

`Try` and `FromResidual` remain deferred. Revisit their public protocol after Rust
stabilizes it, using the stabilized signatures and semantics as the reference.
This checkpoint does not add a custom propagation protocol, generic try_fold,
exception syntax or an Error/cause trait. The executable
[error conversion example](../examples/syntax/error-conversion.kgr) demonstrates
the supported Result behavior.

Validation: 1,236 workspace tests passed, including 40 standalone examples and
159 executable API documentation blocks. Coverage includes source/artifact/JIT
fallback, cross-module generic conversion, once-only evaluation, preserved error
origins under GC, missing bounds, rejected TryFrom/chained conversions, and trap
cleanup. Formatting, workspace/all-targets clippy with warnings denied, and
`git diff --check` passed. This uses the existing v77 execution instructions and
does not change the artifact layout.

## Numeric operations for hardware models

- [x] N01: fixed-width bitwise operations, shifts, compound assignment and const evaluation.
- [x] N02: BitAnd/BitOr/BitXor/Shl/Shr and integer Not static dispatch.
- [x] N03: wrapping, checked, overflowing and saturating integer methods.
- [x] N04: numeric casts, built-in From/TryFrom conversions and typed conversion errors.
- [x] N05: signed offset wrapping and bit rotations.
- [x] N06: 6502 examples, boundary tests, artifacts and final validation.

Ordinary integer arithmetic traps on overflow in every build mode. Explicit
numeric operations follow Rust rules, with 64-bit isize/usize in Kagari. JIT
optimization, compact buffers and fixed-length arrays are separate work.

The executable [6502 numeric example](../examples/6502-numeric.kgr) covers
address assembly, stack and zero-page wrapping, signed branch offsets, RAM
mirroring and ADC/SBC carry/overflow flags. It is a numeric acceptance sample,
not a cycle-accurate CPU implementation.

Final validation: 1,253 workspace tests passed, including 43 standalone examples
and 331 executable API documentation blocks. Numeric acceptance covers source,
encoded artifacts and JIT-enabled interpreter fallback with frequent GC. Tests
compare integer policies and floating-point cast boundaries against native Rust,
reject malformed numeric instruction contracts, preserve compound-assignment
effects on failure, and check direct native argument validation. Formatting,
workspace/all-targets clippy with warnings denied, and `git diff --check` passed.
KBC/runtime ABI v80 rejects earlier products.

## General array operations and ranges

- [x] A01: dynamic repeat arrays with once-only evaluation; see the safety revision below.
- [x] A02: atomic fill and equal-length copy_from on writable arrays.
- [x] A03: independent range values, lazy integer iteration and range declarations.
- [x] A04: copy_within with validated ranges and overlap-safe shallow copying.
- [x] A05: examples, API documentation, artifacts and workspace validation.

These are general language and standard-library capabilities. Fixed-size array
types, compact numeric storage, host buffer exchange and JIT optimization remain
separate work. Range expressions stop materializing arrays; callers that need
storage collect the range explicitly. Array copies retain referenced object identity.

Validation: 1,265 workspace tests passed, including 44 standalone examples and
343 executable standard-library documentation blocks. Range and bulk-copy
acceptance covers source, encoded artifacts and JIT-enabled interpreter fallback
with frequent GC. Tests cover lazy wide ranges, inclusive integer maxima, signed
endpoints, generic RangeBounds inference, custom bound side effects, overlapping
copies, invalid bounds, resource failures, stale artifact rejection and API
navigation. Formatting, workspace/all-targets clippy with warnings denied and
`git diff --check` passed. KBC/runtime ABI v82 rejects earlier products.

Checkpoints: `2e1b50a` implements A01/A02; `719cd5a` implements A03/A04 and
synchronizes grammar, specifications and API examples. This acceptance record
closes A05. Range endpoints currently use builtin integers; borrowed slice views,
range indexing and range comparison/hash protocols are outside this phase.


## Safe array initialization

- [x] Restrict repeated elements to types without shared mutable identities, including recursive Tuple/enum checks and artifact verification.
- [x] Add source-declared ArrayList::from_fn with ordered per-index callbacks and ordinary execution cleanup.
- [x] Replace the shared-object repetition example with independent initialization and explicit sharing examples.
- [x] Complete focused and workspace validation; publish KBC/runtime ABI v83.

This supersedes A01's original allowance for repeating object references. Ordinary
shallow copying and fill retain their established identity semantics.

Validation: 1,269 workspace tests passed, including 44 standalone examples and
344 executable API documentation blocks. New coverage rejects nested and empty
shared-object repetitions, verifies value aggregates, generic initializers,
argument/callback order, zero calls, explicit sharing, callback failure side
effects, cancellation, budget exhaustion and forged repetition bytecode. Source,
artifact and JIT-enabled fallback paths preserve behavior with frequent GC.
Formatting, workspace/all-targets clippy with warnings denied and diff checks passed.

## Read-only map snapshots

- [x] Return List from LinkedHashMap keys/values/entries while preserving ordered,
  independent shallow snapshots; writable copies require ArrayList::from.
- [x] Normalize native snapshot allocation and List interface construction; encode
  KBC/runtime ABI v85 and reject unlowered snapshot bindings before execution.
- [x] Cover inferred readonly results, shallow aliases, writable copies, empty and
  generic snapshots, qualified calls and source/artifact/JIT fallback execution.

Validation: 1,278 workspace tests passed, including executable standard-library
documentation and standalone examples. Formatting, workspace/all-targets clippy
with warnings denied, and diff checks passed. Full tests used an isolated target
directory after a Windows linker file-access failure in the existing build output.

## String joining through collection interfaces

- [x] Declare string-constrained List and Iterator join operations in the bundled
  standard library; preserve the concrete ArrayList string fast path.
- [x] Lower generic joining through ordinary iteration, buffering each string once
  before final allocation. Keep native-default interface slots optional and retain
  declared method ordinals; publish KBC/runtime ABI v86 without compatibility.
- [x] Cover readonly and mutable views, key snapshots, generic and custom sources,
  partial progress, first-None termination, Unicode, empty inputs, type rejection,
  frequent GC and source/artifact/JIT fallback behavior.

Validation: 1,282 workspace tests passed across the final 1,281-test run and the
previously passing executable standard-library documentation test. The final run
skipped only that unchanged documentation test after fixing completion filtering
and the native-witness assertion. All 12 standard API query tests, formatting,
workspace/all-targets clippy with warnings denied and diff checks passed.

## Standard library completion

This sequence extends the source-declared API without borrowed views, ownership
transfer APIs or compatibility aliases. Each checkpoint updates declarations,
runtime/lowering contracts, documentation and executable acceptance cases.

- [x] S01: Unicode trimming, substring search and prefix/suffix stripping.
- [x] S02: Lazy string splitting, bounded splitting, lines and whitespace.
- [x] S03: Replacement, repetition, case conversion and byte/boundary iteration.
- [x] S04: Lazy Option/Result combinators, flattening and transposition.
- [x] S05: FromStr, typed parsing and integer radix parsing.
- [x] C01: Map snapshot interface methods and copy_from naming.
- [x] C02: List endpoint, membership, prefix/suffix and binary search queries.
- [x] C03: List reordering, truncation, prepared extension and swap removal.
- [x] C04: Concrete collection capacity construction and reservation.
- [x] C05: Set relationships and symmetric difference over readonly interfaces.
- [x] C06: Guarded Map get_or_insert_with and update operations.
- [x] C07: Prepared retain, stable sorting and adjacent deduplication.
- [x] C08: Lazy snapshot windows/chunks and immediate range removal.

Callback mutations prepare changes before committing; callback failure preserves
the target's slots/order, while previously completed object side effects remain.
Callbacks cannot modify the target container through aliases. Lazy windows/chunks
produce independent readonly shallow snapshots at yield time. Strings use UTF-8
byte offsets and Unicode scalar iteration, without implicit normalization.

Compact buffers, live sublist views, double-ended iteration, StringBuilder and
generalized Try/FromResidual remain separate follow-up work.

S01 validation: source, serialized-artifact and JIT-fallback acceptance passed
with collection threshold one; definition navigation resolves the documented SDK
member. KBC/runtime ABI v87 rejects previous formats.

S02 validation: lazy split/line acceptance passed on source, encoded artifacts and
JIT fallback with frequent collection. Constructor-forgery checks reject invalid
argument shapes, modes and missing operands. Targeted clippy and diff checks pass;
KBC/runtime ABI is v88.

S03 validation: Unicode expansions/contextual casing, empty and bounded
replacement, repetition, byte and scalar-index iteration pass across all three
execution paths with frequent GC. Artifact/runtime ABI is v89.

S04 validation: 3 combination tests and 16 error-trace tests pass, including
generic payloads, object aliases, lazy branch selection, callback trap cleanup
and original error preservation through nested flatten/transpose. KBC/runtime ABI
is v90; targeted clippy and diff checks pass.

S05 validation: native and custom FromStr, explicit/contextual/generic inference,
all integer widths at their limits, overflow, radix, signs and floating/boolean
syntax pass with source/artifact/JIT fallback. Workspace clippy passes. SDK
implementations and ParseError are source-declared. KBC/runtime ABI is v91.

C01 validation: 24 collection/array tests pass, including native and custom
readonly Map snapshots with non-hashable keys, object aliases, generic calls and
rejection of writes and the removed copy name. Native protocol identities no
longer recursively initialize the contract catalog when one protocol returns
another protocol. KBC/runtime ABI is v92.

C02 validation: native/read-only/custom List queries, generic PartialEq, custom
ordering, insertion positions, empty inputs and Ord rejection pass across source,
artifact and JIT fallback with frequent GC. Artifact/runtime ABI is v93.

C03 validation: 25 array/collection tests pass, including interface reordering,
truncation, prepared self-extension and unordered removal. Failure tests preserve
slots after invalid swap or mutation during iteration. Extend's readonly List
input is snapshotted before the storage commit. KBC/runtime ABI is v94.

C04 validation: 12 capacity/array tests pass, covering all concrete storage
classes, reservation during iteration, order preservation and overflow failure
without modification. Workspace clippy passes. Live heap units still measure
stored values rather than allocator capacity; preparation uses allocation limits.
KBC/runtime ABI is v95.

C05 validation: readonly/native/custom Set operands, generic unbounded relations,
empty/self operations and insertion ordering pass through source, serialized
artifacts and JIT fallback with collection threshold one. Existing collection
interface and standard trait tests pass. KBC/runtime ABI is v96.

C06 validation: lazy Map insertion and updates work through concrete storage and
MutableMap, including custom colliding keys, scalar results and shared objects.
Callback traps and alias writes preserve entries, keep completed external effects,
and release frame guards and roots. Source/artifact/JIT fallback tests pass.

C07 design: native ArrayList sorting uses stable bottom-up merging in explicit
frames, with once-per-element key extraction. Native retain records decisions
before replacing storage, preserving stored hash tokens. These operations belong
to concrete storage: a general user MutableList/Map/Set cannot promise an atomic
bulk replacement using only its individual write methods.

C07 validation: stable order, once-only key callbacks, merge-run boundaries,
empty inputs, live readonly aliases, custom map/set keys and adjacent dedup pass
through source/artifact/JIT fallback. Callback traps and alias mutation preserve
original array slots. Existing array and Map update tests also pass.

C08 validation: lazy snapshot timing, independent slots/shared objects, custom
List indexed traversal, zero/oversized windows, short chunks, fused exhaustion and
early-close guard release pass through source/artifact/JIT fallback. Range removal
covers readonly results, live aliases, empty/full/prefix/inclusive ranges and
failure without target modification. This checkpoint used KBC/runtime ABI v99.
The final iterator-resumption audit advances the format and runtime ABI to v100.

Final completion audit: all S01-S05 and C01-C08 checkpoints are implemented and
committed independently. Window/chunk resumption validates source revisions and
restores guards after early closure. Native API query tests now recognize FromStr
implementations and Set algebra's interface-owned declarations. Current KBC and
runtime ABI versions were v100 at that checkpoint; subsequent entries record
further version changes. Old artifacts are rejected without compatibility.

Validation covers 1,305 passing tests, including Rust doctests and the executable
SDK documentation test. The initial full workspace run passed every SDK example
(source and encoded artifact execution) before exposing two outdated HIR metadata
assertions. After updating only those assertions, the final workspace run passed
1,304 tests and skipped that one already-passing, unchanged documentation test.
Formatting, workspace/all-targets clippy with warnings denied, and git diff checks
also passed. Logs are under target/stdlib-completion-workspace.log,
target/stdlib-completion-final.log and target/stdlib-completion-clippy.log.

### Unified callable protocol

- [x] Source-owned `Fn<Args, Output = R>`, callable-bound shorthand, contextual
  closure inference, and user callable objects.
- [x] Interoperability with `fn(...) -> R` callback parameters through ordinary
  GC-managed closure adapters; source/artifact and interpreter/JIT-fallback tests.
- Deliberately excluded: ownership-based `FnMut`/`FnOnce` distinctions and new
  optimized calling conventions.

Artifacts and runtime ABI advance to v101. Function-typed values automatically
implement `Fn`; user objects are adapted to the existing closure representation.
Validation passes all 1,315 workspace tests with no ignored or filtered tests,
including executable SDK documentation, source/artifact execution, JIT fallback,
GC capture lifetime, trap cleanup, evaluation order, inferred associated outputs,
and analysis-cache reuse. Formatting, workspace/all-targets clippy with warnings
denied, and git diff checks also pass. Logs: target/fn-workspace-final.log and
target/fn-clippy-final.log.
