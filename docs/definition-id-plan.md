# Definition IDs throughout semantic processing

Status: active implementation; SI01 is complete; coupled SI02-SI06 migrations are in progress.
Planning baseline: `2276061e`, after the SA12/SA13 HIR cleanup. The
[roadmap](implementation-roadmap.md) owns sequencing. This plan owns SA19's
implementation scope, SI01-SI06 acceptance and progress ledger.

## Outcome and scope

Use `DefinitionId` throughout semantic records and algorithms. Remove identity
representation parameters from types, declarations, constraints, analysis results,
MIR, bytecode and runtime metadata. Keep exact paths at explicit authoring/display
boundaries and file-local references inside codecs.

```text
Source declaration / Native registration input
    -> register in an explicit definition context
    -> DefinitionId in checking, specialization and executable metadata
    -> borrow names/paths for diagnostics and explicit external queries
    -> encode file-local references; decode and validate into scoped IDs
```

This changes Rust APIs and internal representations, not Kagari syntax, generic
semantics or execution behavior. Language type parameters and associated types
remain; only the Rust parameter choosing an identity representation disappears.
Do not rename the semantic `TypeId` enum or replace it with a type interner here.

Scope includes input adoption, Native catalog validation, HIR queries and caches,
specialization, executable verification, portable encoding and runtime adoption.
Migrate affected Host declaration consumers to IDs where needed, without merging
Host and Native registration or changing scoped host access.

Exclude SA9's full module-graph handle migration, SA11 declaration self-ID removal,
SA15 import syntax segments, SA18 collection access redesign, SA21 registration
unification, native collection storage migration and new language features.
Snapshot container optimization, specialization scheduling and runtime code-image
sharing are separate measured follow-ups. Do not introduce a global identity pool.

## Baseline evidence

| Owner | Current behavior and required change |
| --- | --- |
| Original identity-reference abstraction (removed in SI06) | `DefinitionReference` previously abstracted paths, IDs and file references. Scoped visitors/remappers and codec-owned wire records replace it; no shared semantic representation abstraction remains. |
| [Definition tables](../crates/kagari-common/src/identity/table.rs), [context/maps](../crates/kagari-common/src/identity/map.rs) | Eight-byte table/index IDs, append-only interning, borrowed views, explicit imports and immutable snapshots already exist. Reuse them; make internal map access ID-based rather than materializing and sorting paths. |
| [Metadata ownership](../crates/kagari-common/src/identity/metadata.rs) | `DefinitionMetadata` retains a table and validates references; generic rebinding supplies paths, wire data and cross-context imports. Preserve ownership/reference validation, separate the three conversion responsibilities. |
| [HIR types](../crates/kagari-hir/src/types.rs), [declarations](../crates/kagari-hir/src/declarations.rs) | Semantic records default to paths while published caches retain IDs. Register declaration identities before semantic processing. |
| [Analysis ownership](../crates/kagari-hir/src/analysis/ownership.rs), [body queries](../crates/kagari-hir/src/analysis/body_queries.rs), [method queries](../crates/kagari-hir/src/analysis/method_queries.rs) | Cached facts are projected to paths for checking, reuse comparisons and completion. Move those consumers to IDs with explicit table access. |
| [Native builder](../crates/kagari-runtime/src/native/builder.rs), [catalog ownership](../crates/kagari-runtime/src/native/catalog/ownership.rs) | Builder inputs and several validation/dependency queries use path records, although installed declarations use IDs. Adopt inputs earlier and validate scoped records directly. |
| [Program specialization](../crates/kagari-compiler/src/source/program.rs) | Checked modules are projected through `to_unverified` into path records before specialization. Retain scoped identities while dropping verification evidence only where mutation requires it. |
| [KBC artifact](../crates/kagari-bytecode/src/artifact.rs), [wire identities](../crates/kagari-common/src/identity/table/wire.rs) | KBC decoding maps file references into IDs and then back into paths. Decode to scoped unverified records directly, followed by normal contract validation. |

The root issue is that an identity representation choice propagates through
semantic APIs and forces whole-record conversions. Existing tables and validation
are useful foundations. This inspection establishes conversion work, not a measured
end-to-end speedup or a present soundness failure.

## Identity ownership and invariants

Keep `DefinitionId`'s private table identity and index. An analysis context shares
one append-only writer; published results retain immutable table snapshots.
Native module construction and runtime adoption may own independent contexts.
Import foreign definitions explicitly before combining their semantic records.
Do not attach a table/Arc to every type node: the enclosing analysis, catalog,
program or installed metadata owner retains the matching table.

- Appending does not renumber existing IDs. A retained snapshot resolves its
  existing prefix and rejects newer or foreign IDs outside that snapshot.
- Context clones share the existing writer. Independent writers must never assign
  different definitions the same table/index identity.
- Equal paths in different contexts may have different IDs. Import the referenced
  ancestor closure once per adoption operation, then remap all identity fields.
  Same-context comparisons use IDs; cross-context contract comparisons first
  normalize into a common scope or use an explicit boundary comparison.
- Exact identity includes module, definition kind, name and occurrence. Preserve
  generic owner/position identity, canonical aliases and source shadowing rules.
- Interning a path establishes an identity, not declaration existence, visibility,
  type correctness, provider authority or executable verification.
- Definition identity is separate from HIR arena/body IDs, source revisions and
  runtime generations. An unchanged ID does not prove an unchanged signature,
  cache entry, implementation or supplying generation after reload.
- Reuse definition-table module roots and shared path storage. Record how a future
  SA9 graph handle would query that owner; do not create a second module interner
  or substitute lowering-local `ModuleId` for an analysis-universe module handle.

Preserve current finite table/path limits, cancellation and fallible exhaustion.
Never weaken reference validation or introduce unchecked constructors to ease
migration. Adding an interned identity after a failed operation may be harmless;
publishing partially validated semantic records is not.

## Declaration, analysis and Native inputs

For a source declaration, collect its identity before checking its signature or
body, preserving forward references and existing declaration disambiguation.
Unresolved source names remain syntax/resolver data; diagnostics and error types
represent failed resolution rather than inventing a valid declaration.

```text
struct Box<T> { val value: T }

Box header       -> box_id
value member     -> value_id, with box_id as its definition parent
generic T        -> owner box_id, parameter position 0
Box<i32>         -> Struct(NominalType {
                      declaration: box_id,
                      arguments: [Builtin(I32)], associated_types: {}
                   })
field selection  -> checked member/type facts, without rebuilding Box's path
```

Remove identity parameters from the final semantic types, substitutions, bounds,
signatures, catalogs and analysis records. Give name/kind/parent queries their
owning table explicitly; algorithms that only compare IDs need no display context.
Use borrowed table views for diagnostics and navigation where possible. Materialize
an owned path only when an external API actually requests one. Avoid optional table
arguments whose meaning depends on the current identity representation.

Native builders own or share an explicit construction context and import provider
identities before use. Defining a type/function creates its ID; method, field,
bound and binding records refer to that ID. Preserve typed builder handles and
their ownership checks. String/path-taking APIs remain input conveniences that
resolve at entry, not alternate semantic engines. Any necessary unprocessed
authoring input belongs at that boundary and must not duplicate all semantic models.
Replace unpublished path-record construction APIs and callers coherently.

Catalog implementation checks, receiver ownership, dependency closure and exact
binding/storage admission query IDs and tables directly. Preserve source-free
validation; runtime consumers must not acquire a HIR/source-analysis dependency.

Analysis cache comparisons retain source revision, imports, language-item/provider
contracts and cancellation dependencies. Do not replace those checks with ID
equality alone. Member completion and navigation return identities tied to the
retained snapshot, or explicitly exported paths for external consumers.

## Verification, remapping and file representation

Keep reference-scope validation distinct from semantic verification. Unchecked and
checked records both use IDs; immutable checked wrappers continue to own evidence.
Extracting editable records discards that evidence while retaining their identity
scope. Modification must still pass the existing verifier before execution.

Retain a focused reference visitor and fallible ID-to-ID remapping facility at
record owners. Replace arbitrary `Rebind<J>` with the operations actually needed.
Mappings must cover nested types, generic owners, associated members, call evidence,
metadata and keys. Reject collapsed keys where uniqueness is required, and preserve
semantic ordering of parameters, fields, instructions and type arguments.

Use codec-private wire records for actual serialized metadata, with explicit
conversion to/from the fixed-ID semantic model. Share wire type definitions at
their existing dependency owner where KBC and MIR need the same representation;
do not create duplicate semantic algorithms or a new forwarding crate. Do not
mirror nonserialized HIR caches/inference state into a second record hierarchy.
Encoding may use borrowing views where justified, but no global/thread-local
serialization context or standalone Serde codec for process-local IDs is allowed.

```text
Encode: scoped records + table
        -> reachable identities/ancestors in canonical order
        -> file table + local references + wire records

Decode: bounded file input
        -> validated file table/references
        -> scoped unverified records + retained table
        -> existing semantic/program verification and installation checks
```

Retain canonical bytes/fingerprints across table numbers, insertion orders and
unrelated interned definitions. Audit ID-keyed map traversal: numeric order must
not accidentally become artifact order, overload priority or diagnostic policy.
Retain byte/count/depth limits, duplicate/cyclic/forward-parent rejection, reference
bounds, ABI/schema checks and native-input correspondence checks. Serialized data
never carries an executable seal. Loading must work without source compilation.

Runtime imports normalize into their owning context while retaining supplying
versions, dependency closures, roots and pinned calls. Do not equate an ID match
with reload compatibility or change code/data lifetime during this migration.

Development artifacts may be invalidated and regenerated at coherent checkpoints.
Keep format/ABI identifiers; no routine bump or old-reader compatibility layer is
required for this unpublished refactor. Preserve meaningful source-free fixtures.

## Execution phases

| Phase | Scope and exit condition |
| --- | --- |
| SI01: boundary inventory and identity services | Trace path/scoped/wire consumers and define their final owner, including metadata validators and public constructors. Settle Native provider adoption, table queries, visitors/remapping and codec-private record placement. Record exact deletion targets and selected baseline tests/workloads here. Reuse existing table/context behavior and implement only missing boundary primitives. |
| SI02: scoped declarations and Native registration | Register source declaration identities early; migrate shared declaration validation and Native builder/catalog/installation consumers to IDs. Import providers explicitly. Demonstrate generic builder references, exact binding rejection and cross-context adoption through existing APIs/tests adapted to the new contract. |
| SI03: scoped HIR semantics and queries | Migrate signatures, body checking, substitutions, constraints, aggregates, reuse comparisons, completion and navigation. Eliminate whole-analysis authoring projections on those paths. Preserve incomplete-source recovery, canonical aliases, generic owners and retained snapshots. |
| SI04: scoped specialization and executable checking | Carry IDs through specialization, MIR/bytecode generation and verification; mutable extraction retains scope but drops verification evidence. Migrate directly affected backend consumers without changing their supported feature sets. Preserve checked calls, associated/default methods and supplying versions. |
| SI05: portable codecs and runtime adoption | Introduce the bounded private wire conversions; decode directly to scoped records and revalidate. Migrate artifact/native-input/runtime adoption and affected Host metadata consumers; remove path round trips at these boundaries. Prove canonical encoding, forged-input rejection, source-free loading and pinned reload. |
| SI06: remove obsolete representations and integrate | Remove `DefinitionReference`, identity representation generics, arbitrary rebinding and whole-record path projections after all consumers move. Replace obsolete APIs/tests/docs, refresh affected fixtures once, complete selected dependent checks and summarize any measurements. Move lasting contracts to architecture/specifications and retire this plan. |

Phases describe responsibility order, not permission to leave broken consumers
until the end. Prefer buildable checkpoints; change coupled signatures, validators
and callers together. Existing generic representations may temporarily serve
unmigrated callers, with removal owned by SI06; do not add new path-model variants,
compatibility aliases or forwarding APIs. Move a necessary codec/caller adjustment
earlier when required for a coherent checkpoint and record it here.

If an inseparable boundary cannot build yet, attempt the relevant scoped check and
record command, representative diagnostics, cause and owning follow-up phase in the
ledger and any commit body. Do not repeat an unchanged known failure before its
owner changes. No production stubs, disabled verification or weakened assertions.
Track phase progress separately from build status; final integration permits no
carried errors. Implementation commit trailer: `Plan: definition-id-plan SI0N`.

## Verification and acceptance

Reuse existing contract owners; add cases only for uncovered behavior, not for each
removed wrapper or renamed field. SI01 selects test names from these owners for
each checkpoint, rather than running every listed target in full.

| Boundary | Existing owner and selected evidence |
| --- | --- |
| Table scope and portability | [Table tests](../crates/kagari-common/src/identity/table/tests.rs): foreign/absent IDs, retained prefixes, kind/occurrence distinctions, ancestor import and canonical ordering. |
| Analysis and recovery | [Identity tests](../crates/kagari-hir/src/analysis/identity_tests.rs), [method queries](../crates/kagari-hir/src/analysis/method_queries/tests.rs): owner-qualified generics, aliases/navigation, declaration reordering, rejected-code recovery and retained snapshot validity. |
| Native input boundary | [Builder tests](../crates/kagari-runtime/tests/native_builder.rs), [registration sources](../crates/kagari-embed/tests/registration_sources.rs): provider imports, generic members and wrong/missing binding rejection. |
| Specialization and dispatch | [Default methods](../crates/kagari-embed/tests/default_methods.rs), [associated types](../crates/kagari-embed/tests/associated_types.rs): cross-module/default/override and generic/associated-member selection through source and artifact execution. |
| File verification | [Compiler codec tests](../crates/kagari-compiler/src/tests/codec.rs), [artifact features](../crates/kagari-embed/tests/artifact_features.rs): canonical round trips, reconstruction of seals, malformed references/counts, cancellation and source-free execution. |
| Runtime versions | [Generic reload](../crates/kagari-embed/tests/generic_reload.rs), [native artifacts](../crates/kagari-embed/tests/native_artifacts.rs): old closures retain types/operations and rejected reload preserves the previous installation. |

Useful initial filtered commands, to select by changed boundary rather than run
as one mandatory sequence:

```text
cargo test -p kagari-common --lib identity::table::tests
cargo test -p kagari-hir --lib published_cache_scopes_retain_prefixes_and_reject_foreign_queries
cargo test -p kagari-hir --lib generic_parameter_identity_is_owner_and_position_based
cargo test -p kagari-compiler --lib portable_program_codec_rebuilds_seals_and_preserves_canonical_lowering
cargo test -p kagari-embed --test default_methods generic_implementations_and_default_local_binders_are_specialized
cargo test -p kagari-embed --test generic_reload retained_generic_default_and_override_closures_pin_types_and_operations_after_reload
```

Run affected checks/tests plus a small directly dependent regression set; complete
skipped cases after build failures and use `--no-fail-fast` for multiple selected
targets. Reuse passing evidence for unchanged code. Whole-workspace testing and
the complete feature/backend matrix belong exclusively to CI, including final
acceptance. Select a source-free loading boundary locally; do not infer actual
native execution from a successful interpreter fallback.

At implementation checkpoints review handwritten imports, visibility, ownership,
re-exports and effective LOC; run `uv run --locked scripts/check_structure.py`,
formatting and `git diff --check`. Scope Cargo/Clippy to affected consumers.
Documentation-only planning needs content/link/diff checks, not builds.

SI01 records a bounded repeated analysis/query or specialization workload if
performance comparison is undertaken. Measure path materialization/allocation
cost and elapsed processing separately from compilation; record machine, toolchain,
profile/features, parallelism, cache state and competing load. No speed or retained
memory claim follows from deleting generics alone. Measurement difficulty does not
justify adding unrelated caches or weakening semantic acceptance.

Final local acceptance requires the selected contracts to pass, no remaining
semantic identity representation parameter/path projection, and an audit that
remaining path/file-reference uses belong to explicit boundaries. Report pending
CI and any unmeasured performance claims separately. Source spelling, diagnostic
text, portable module identity and explicit requested path exports are legitimate
boundary data; do not erase them to satisfy a text-search count.

## Progress and error ledger

- [x] Agree the representation/ownership design and record this execution plan.
- [x] SI01: Boundary inventory and identity services.
- [ ] SI02: Scoped declarations and Native registration.
- [ ] SI03: Scoped HIR semantics and queries.
- [ ] SI04: Scoped specialization and executable checking.
- [ ] SI05: Portable codecs and runtime adoption.
- [ ] SI06: Remove obsolete representations and integrate.

### SI01 boundary decisions and validation

The baseline inventory finds DefinitionReference consumers in ten crates: common,
types, contract, HIR, MIR, bytecode, runtime, stdlib, VM and embed. There is no new
identity owner: common retains context/table/import mechanisms; each semantic
crate owns traversal/remapping of its records. Native builder/provider adoption
and source declaration collection must register IDs before their respective
semantic algorithms run. Their outputs retain the owning table/context.

Deletion targets are the representation trait/description union in common,
`DefinitionRecord::Rebind` and arbitrary representation mapping, metadata
`to_paths`/`scope_record`, HIR ownership `paths`/`scope` projections, catalog
`collect_authoring`/`scoped`/`to_paths`/`CatalogScopes`, and path-based editable
MIR/KBC extraction. Keep reference validation, ID-to-ID imports and loss of
verification evidence on mutable extraction. Remove each obsolete helper only
with its final caller; existing generic records remain an SI06 removal obligation.

Shared wire leaf types belong with kagari-types' existing bounded flat type
codec; shared executable wire records belong with their kagari-contract owners.
MIR and KBC envelope records remain in their codec modules. Cross-crate codec
access, where necessary, is an explicit codec API without broad re-exports or
semantic algorithms. Preserve the flat depth/node-bounded decoder rather than
replacing it with recursive derived decoding. Fingerprinting/serialized-size
consumers must use explicit canonical boundary data; no process ID Serde.

DefinitionTable already supplies parent/child lookup, checked resolution and
ancestor import. Added direct module/child interning through the shared
DefinitionContext writer, checked DefinitionMap insertion by ID, and a borrowed
final-segment query. The path-taking insertion boundary delegates to checked ID
insertion. Extended existing scope tests to cover rejected foreign insertion
without mutation, shared-writer child allocation and old snapshot validity.
No new record hierarchy or duplicate registry was introduced.

Validation: `cargo test -p kagari-common --lib identity::` passed 14 tests before
and after these changes; `uv run --locked scripts/check_structure.py` passed
(1035 files, zero violations/exceptions). These checks select the changed identity
boundary and do not establish downstream migration acceptance. No performance
measurement or speed claim is made.

### SI02 current slice

Native catalog collection now interns type/trait keys and adopts each declaration
into the catalog's ID context directly. Removed the intermediate whole authoring
catalog collection and subsequent `scoped` pass. Insertion checks compare scoped
records under checked ID keys and retain conflict/implementation-count checks.
Path-taking registration inputs, proof validation and typed builder handles still
require migration; SI02 is not complete.

Native declaration validation now takes fixed-ID records plus their retained
table. Binder ancestry, callable parent/kind/name and binding identity queries use
that table. Added bounded canonical type comparison, including associated-member
map ordering, so ID allocation order cannot change sorted-bound acceptance.
Existing path and scoped callers share the bounds/constraint checking algorithm;
no second native declaration validator was retained. Runtime registration validates
its existing scoped declarations directly.

Temporary caller adoption is explicit in ModuleDecl validation, ProofCatalog,
MIR/bytecode verification and the compiler ABI test. These callers still own path
records and invoke the existing scope_record boundary. This adds temporary
conversion work, not a performance improvement: remove ModuleDecl/ProofCatalog
adoption in SI02 and executable/test adoption in SI04 when their owners retain IDs.
Rust-authored codec inputs still carry paths until builder migration; admission
resolves those inputs in the retained signature table instead of exporting the
checked signature to paths. Link-time NativeImport structural validation still
projects its whole import for the old contract checker; SI04 must migrate that
checker and remove program.paths(import).

The native catalog and dependency closure now have one fixed-ID representation.
Removed the path catalog implementation, whole-catalog to_paths, CatalogScopes,
scope_values and their two-representation import cache. Closure seeds retain the
catalog context; extending a foreign seed is rejected. Traversal visits every
matching implementation, so numeric ID iteration cannot select an overload;
linked proof inputs still use canonical path order at their temporary boundary.
NativeModule groups scoped declaration entries before dependency selection.

Registration installation imports declarations once into the runtime context,
validates exact IDs, checks codecs against scoped signatures and inserts ID keys.
Owner publication imports keys for types, traits and local storage without owned
path reconstruction. CatalogImports now belongs to the entire staged installation
batch; it retains source Arcs, so cache keys cannot be reused after a drop.
The staged registry is still published only after the whole batch succeeds.

ImplementationPattern receives the protocol role resolved by the identity owner.
It uses the existing matching/substitution algorithm for scoped inputs and remaining
path callers; its temporary identity parameter is removed with the shared model
in SI06. Collection-access matching is unchanged. There is no optional display
context or second matching implementation. ProofCatalog, receiver-owner validation
and authored implementation validation still receive explicit path-record inputs
at their existing boundaries; migrate these in the remainder of SI02.

Next coupled boundary: migrate the remaining shared declaration/receiver ownership
checks, Native builder handles/provider adoption and ProofCatalog to their retained
ID contexts. Do not introduce a second scoped proof engine or bypass existing
validation. Preserve bounded search/cancellation budgets
and exact provider/receiver ownership checks.
Builder/provider references must be adopted into the builder context before
signature construction; scalar types carry no definition references, while foreign
nominal/generic references require explicit import or rejection. Owning builder
handles may retain their context; internal type nodes must not acquire per-node
context Arcs.

Selected validation: runtime `native::module::tests::` covers independent provider
contexts, retained registrations, exact dependency closures and installation
failure isolation. Builder selections will cover generic receiver groups and
wrong/missing method/entry rejection. The six runtime module tests and four selected builder tests passed (generic
receiver binders, missing/unknown trait members, missing entries and selected-call
bound rejection). `cargo clippy -p kagari-common -p kagari-runtime --lib -- -D warnings`
passed, as did structural/formatting/diff checks. The final direct impl-child registration adjustment also passed a rerun of
the affected generic receiver builder case. No build failure is currently carried. Later phases select the HIR owner/snapshot tests,
compiler codec rejection/canonical tests and one generic default/reload route from
the existing coverage table. The complete matrix remains CI-owned.

The fixed-ID validator slice passed 22 selected tests: eight common table tests,
three scoped declaration/type-order tests, four native builder cases listed above,
six runtime module ownership/adoption tests, and compiler
`every_installed_callable_and_public_contract_passes_portable_validation`.
The latter covers the shared path/scoped bounds helper through installed APIs.
The initial test-only borrow-check errors were repaired before these runs; no
failure is carried. Scoped library checking passed for runtime/MIR/compiler, and
Clippy passed for types/runtime/compiler. Structural checking found zero violations
in 1036 Rust files. Formatting, documentation links and diff checks passed.
Exhaustive workspace/backend/feature testing remains CI-only; SI02 and final
acceptance are still incomplete. No performance result is claimed for this slice.

The catalog/installation slice passed 17 selected tests: all six native module
scope/dependency/isolation cases; six native_builder cases (explicit exact codecs,
opaque-storage rejection, read/write receiver groups, generic receiver binders,
generic defaults and invalid default codecs/obligations); the two types collection
access tests; the two contract implementation matching tests; and
`cargo test -p kagari-embed --test native_provider_reset external::an_embedding_owned_provider_can_allocate_and_chain_checked_callbacks -- --exact`.
That artifact test constructs a fresh consumer engine, loads decoded bytes, runs
native allocation/callbacks and checks roots, depth and final GC reclamation.
The intermediate matcher type mismatch and a test-only type inference error were
resolved; no build failure remains. Types/contract/runtime library Clippy,
formatting and structural checking passed (1037 Rust files, zero violations).
The complete workspace/backend matrix remains CI-owned; no speed claim is made.

Update this ledger with validation results, deviations and owning follow-ups; do
not create another progress queue. On completion, move contracts to their
architecture/spec owners and remaining work/CI status to the roadmap, then remove
this plan.

### SI02 receiver ownership checkpoint

`ReceiverOwners::owner` now requires a fixed-ID receiver and its retained table.
Nominal ownership resolves that ID in the supplied scope; an equal path in another
context, or an ID newer than the retained prefix, does not establish ownership.
The intrinsic scalar/Array rules and declaration-supplied constructor ownership
are unchanged. Catalog owner collection visits ID-keyed entries and borrows their
module identities, removing its whole-type path projection and canonical path sort.
Constructor shape checking shares the existing field/variant/arity algorithm for
both remaining authoring declarations and scoped declarations; its temporary Rust
identity parameter is removed with `TypeDef` in SI06.

ModuleDecl, builder inputs, HIR native import and executable proof records still
carry paths at this checkpoint. Their receiver query explicitly adopts the input
before invoking the single scoped ownership algorithm. These are temporary caller
adapters, not a second ownership implementation. Remove runtime registration and
builder adapters with SI02, HIR native import adoption with SI03, and the contract
proof/ABI and bytecode adapters with SI04. The proof adapter propagates cancellation
and invalid identity conversion separately. Whole ModuleDecl validation and
ProofCatalog migration remain the next coupled work; SI02 is still incomplete.

Selected validation passed 12 tests: the two types receiver-ownership cases,
contract `nominal_equality_overrides_disable_defaults_and_require_owned_explicit_prerequisites`,
six runtime module scope/dependency/installation cases, and three native builder
cases (`builtin_inherent_methods_require_the_language_owner_even_for_raw_declarations`
and the two `receiver_groups` cases). These exercise the changed ownership contract
and immediate registration/proof consumers. Runtime/compiler library checking and
types/contract/runtime library Clippy passed. Structural checking passed with 1037
Rust files and no violations or exceptions; formatting and diff checks passed.
No build failure is carried. Existing source-free/reload evidence is unchanged;
SI04-SI06 must reselect those contracts when their records and codecs migrate.
Full workspace/backend/feature validation remains CI-owned. No performance result
is claimed; remaining path callers now perform temporary input adoption.

### SI02 scoped proof and call-contract checkpoint

`ProofCatalog` now borrows fixed-ID implementation, trait, enum and Native
declaration records. The caller supplies its construction context; the catalog
validates every incoming reference against the input snapshot and retains its own
snapshot. It interns implicit language identities and declared member identities
in that writer before freezing the snapshot. This establishes identity only:
declaration authority, applicability, bounds, default templates, selected calls and
receiver ownership still use the existing bounded, cancellable proof algorithms.
Native catalog proof construction now borrows installed ID records directly,
removing its whole-catalog path projections. Default selection resolves the actual
registered template without requiring an implementation-owned member for inherited
defaults.

This coupled boundary also migrates generic bodies, interface/shared calls,
witnesses, semantic slots, native imports and primitive/protocol contracts to
fixed-ID checking with mandatory table access. Those parts of SI04 were pulled
forward because leaving their contract algorithms path-based would require a
second proof engine or repeated whole-record conversion. Comparison-only
substitution/matching helpers temporarily retain the existing identity parameter;
SI06 removes it with the semantic models. This is within the planned migration,
not a new compatibility layer.

Host function signature validation now operates on IDs and explicitly checks the
retained table, method parent, nominal kind and canonical Option/Result identity.
Existing bounded shape, borrowing and effect validation remains. Host authoring
and fingerprint/codec entrypoints temporarily adopt their input before that single
validator; malformed identity errors remain distinct from size exhaustion.
ModuleDecl's two bounds-validation callers also adopt their input temporarily.
Remove ModuleDecl/builder adoption with the remainder of SI02 and Host wire/input
adoption with SI05; do not keep these as permanent path semantic APIs.

Focused local validation passed 14 distinct types tests: three scoped declaration
validation cases, three Host decode/path-limit cases, seven substitution cases and
the scoped Host signature test covering foreign tables, forged constructor kinds
and malformed authored identities. The path-limit case was rerun after correcting
adoption error classification. The new Host test initially expected malformed
paths to survive adoption; reference validation correctly rejected them earlier,
and the assertion now checks that boundary. Types/contract library checking and
`cargo clippy -p kagari-types -p kagari-contract --lib -- -D warnings` passed.
Structure checking passed with 1038 Rust files and no violations or exceptions;
formatting and diff checks passed. No performance claim is made.

Integration is deliberately incomplete at this checkpoint:

- `cargo check -p kagari-runtime --lib` fails in kagari-bytecode with 77 errors from
  old path callers of the fixed-ID contract APIs. Representative diagnostics are
  the missing `SemanticSlots::protocol_adapter_valid` method for path records in
  `access.rs:77`, path types passed to scoped normalization in
  `trait_bounds/associated.rs:86`, and the scoped import/table signature required
  by `verify_native_call` in `verifier.rs:952`. SI04 owns executable-record and
  verifier caller migration; SI05 owns the associated decode/adoption boundary.
  Runtime changes beyond that dependency failure have not compiled.
- `cargo check -p kagari-hir --lib` fails with two errors in
  `aggregates/protocols.rs`: the path-based adapter request lacks the mandatory
  table and its result consumer expects a path signature. SI03 owns the HIR
  context and record migration. Do not convert the scoped result back to paths.
- Contract proof/default tests and runtime/source-free/reload tests selected for
  this changed boundary remain pending because their test builds depend on these
  unmigrated callers. Earlier successful runs do not validate this checkpoint's
  changed APIs. MIR/compiler/backend integration is also pending; the complete
  feature/backend matrix remains CI-owned.

Continue through those caller owners and finish builder/ModuleDecl scope adoption;
SI02 is not complete. During specialization/codec migration, audit derived bound
ordering: the temporary generic substitution helper uses identity ordering, while
scoped declaration validation preserves canonical path order through the table.
Canonical wire output must not depend on allocation order; retain the existing
reordered-allocation coverage and fix the owning ordering operation rather than
weakening validation. No structural exceptions or new progress queues were added.

### SI02 declaration and SI04 executable-validator prerequisites

Inspection of the bytecode caller failures exposed the remaining shared path
validators underneath that owner. Migrated their existing algorithms before
adapting bytecode: declaration binders/functions/traits/aggregates, conversion
adapter shape, public ABI and carried trait contracts, linked type applications,
aggregate layouts, inherent method signatures, and range/iterator operands now
check fixed-ID inputs. Identity-sensitive operations require an explicit retained
table. Removed the path-building declaration `owner` helper. Scalar constant and
aggregate-shape helpers still share their identity-independent algorithms with
remaining authoring callers; their temporary representation parameters remain an
SI06 removal obligation.

Declaration and public-ABI owners now look up already collected identities rather
than manufacturing paths while checking. `DefinitionTable::lookup_root` reads the
existing module index; it creates no second module registry. Callers must register
the module root and all declared names, including methods whose signatures contain
no nominal references, before retaining the validation snapshot. SI04/SI05 must
preserve that collection step for editable programs and decoded records, including
modules with no public items. Registering these names does not establish valid
declarations or executable evidence.

Native authoring `ModuleDecl::validate` now adopts its declaration batch once and
uses that snapshot for function/trait/type and bounds validation. Removed its
per-implementation, per-method and final native-template `scope_record` calls.
The final derived native template records resolve in the same scope. A temporary
private `module/adoption.rs` collects authoring identities, and `implement_trait`
uses that boundary for its incoming contract. The remaining ModuleDecl construction
and implementation algorithms still carry paths. Replace this temporary boundary
when SI02 moves Native builders and ModuleDecl itself into their retained context;
do not retain an authoring-semantic shadow model in SI06.

Bounds validation continues to compare canonical identity content, including
associated members, rather than numeric ID order. Inherent export matching merges
bounds in the shared scope and explicitly restores canonical ordering before exact
template comparison. Layout validation retains exact child parent/kind/name and
occurrence checks, generic substitution, finite slot limits and cancellation.
Parameter/Self references now also require resolution in the supplied scope;
merely listing a foreign ID as a binder does not authorize it. ABI versions and
wire formats are unchanged.

Focused local validation passed the common borrowed-identity query case and four
types scoped-validation cases. Extended the existing native declaration case to
exercise the fixed function/bounds validator against the same canonical-order and
ownership mutations. The added trait case covers retained-prefix rejection,
foreign tables, wrong associated-member parents, and Native authoring adoption of
implicit method identities. Its test-only missing import during review was fixed
and the affected case reran successfully. Types/contract library checking and
Clippy passed; structure checking reports 1039 Rust files and zero violations or
exceptions. Formatting and diff checks passed. These are core-validator checks,
not executable integration acceptance.

`cargo check -p kagari-bytecode --lib` now reports 151 errors at the unmigrated
path-record callers (superseding the preceding 77-error diagnostic). Besides the
existing scoped-proof/call failures, representative new sites are
`access.rs:116` (`executable_signature_matches` requires ID records and a table),
`program.rs:106` (layout ABI checks), and `trait_bounds/applications.rs` (fixed-ID
application lookup callbacks). SI04 owns these caller/record changes and SI05 the
decode/adoption context. Runtime, contract tests with the stdlib dev-dependency,
MIR/compiler and source-free/reload validation remain blocked behind that boundary;
their existing fixtures also need to supply the retained scope, not restored path
semantic APIs. The previously recorded two HIR adapter errors remain carried;
that unchanged check was not repeated. Complete Host interface contract migration
and executable ownership/adoption next, alongside the remaining SI02 builder
work. No phase is newly marked complete, and the exhaustive matrix stays in CI.

### SI02 Host contracts and SI05 KHI boundary

Host type/member validation, method-contract construction, interface validation,
path resolution, linked Host trait matching and dependency queries now consume
fixed-ID records. Table access is explicit for identity-sensitive operations;
same-scope binding comparison and intrinsic scalar constraints keep their shared
identity-independent algorithms. Removed per-function/per-method path adoption
from Host validation. Method and field parents, nominal kinds, trait outputs,
borrow modes, effects, reference closure, path access and dynamic index slots are
still checked by their existing owners. Host path duplicate detection compares
scoped records directly; it no longer serializes records as a semantic equality
operation.

Standalone KHI encoding now writes a canonical file-reference record envelope and
its referenced definition table. Decoding imports references into a fresh scope,
validates the Host contracts and returns `DefinitionMetadata<HostInterface<DefinitionId>>`.
It does not project the decoded schema back into authoring paths. Function, field,
method, type and value fingerprints encode the referenced closure canonically;
documentation remains excluded from ABI fingerprints. Path resolution uses those
scoped fingerprints without creating intermediate authoring declarations.
Declaration order, table allocation order and unrelated definitions do not change
the output. Byte/node/depth/sequence limits and trailing-data rejection remain in
the codec. KHI's existing identifier stays at 12 under the unpublished policy;
old development products are disposable, with no legacy reader or ABI bump.
The lasting standalone contract is updated in the Host interop specification.

Host record representation parameters and shared Serde implementations remain
temporary until SI06 introduces the final codec-private models. Path-taking
constructors remain authoring inputs, while production semantic/fingerprint APIs
now require adopted IDs. Existing Host fixtures use test-only explicit adoption
and path export for schema comparisons; direct decode/adoption assertions retain
and query the actual tables. These fixture helpers are not a production path
semantic API. SI02 runtime Host registration and SI05 embedded KBC/MIR users must
now pass their retained scopes rather than restore the removed methods.

Focused validation passed 17 distinct Host tests: all 16 existing Host contract,
path, fingerprint and decode-limit cases, plus a file-reference rejection case.
The canonical encoding case additionally checks decode/re-encode, explicit import
into a context with unrelated definitions, stable fingerprints across contexts,
and rejection when the decoded records are checked against a foreign table.
Malformed-envelope fixtures now encode the current file-reference shape before
corruption, retaining their actual rejection boundary. The path duplicate case
reran after replacing serialized equality with record equality. Test-only import
and fixture-method migration errors were repaired before these runs.
Types/contract library checking and Clippy passed. Structural review resolved a
qualified path and missing trait-signature spacing without exceptions; the checker
reports 1041 Rust files. Formatting and diff checks passed. No speed or retained
memory claim is made.

`cargo check -p kagari-bytecode --lib` reports 159 unmigrated caller errors,
superseding the previous 151-error diagnostic. Representative Host sites are
`program.rs:157` and `verifier.rs:275`/`:283`, where scoped Host/public records and
the definition table are now required. Bytecode record ownership, verification,
decoding and mutable extraction remain SI04/SI05 work; downstream runtime, MIR,
compiler and source-free/reload tests have not passed this boundary. The unchanged
HIR adapter failure remains carried. Continue with the executable program's
identity owner and callers, then Native/Host registration and HIR; do not treat
this standalone codec checkpoint as completed SI02 or SI05 acceptance.


### SI04 bytecode verification and SI05 KBC boundary

Migrated bytecode programs, modules, functions, instructions and envelope records
to scoped-ID defaults. Physical instruction checks, access-flow checking, linked
trait applications, method/parent/view selection, call witnesses, Host calls,
layout checks and Native declaration validation now use those records and their
explicit table. Removed bytecode's receiver/native per-record path adoption.
Shared executable trait/type lookup and implementation matching now require IDs;
the old path lookup wrappers are removed. Numeric signatures and array repetition
checking likewise consume scoped semantic types. Numeric operations return one
named operand/result contract instead of an expanding representation tuple.

The verification entry checks every existing reference against the initial context
snapshot before collecting implicit module/public/member names. The collector
assigns identities without granting ABI validity; existing module and linked proof
checks still establish that contract. Frozen verification evidence owns the exact
records and a retained immutable table. `to_unverified`/`into_unverified` now return
`DefinitionMetadata<BytecodeProgram>` directly, dropping only the seal. The metadata
owner can transfer records and table together through `into_parts`. KBC decoding
returns scoped metadata directly, without `to_paths`; cross-context adoption is an
explicit complete import. Encoding, content hashes, dependency fingerprints, public
ABI identities and required Host fingerprints use canonical referenced closures.
The entire envelope's references and combined encoded size are checked before
semantic verification/hash acceptance, including identity-bearing verification
summaries outside the program. KBC version 116 and runtime ABI identifiers remain
unchanged under the unpublished policy.

The raw record types and generic wire mapping remain temporary SI06 work. Bytecode
constructors/verification currently receive an explicit append-only context;
decoded and extracted records retain frozen ownership and must be imported into a
writer before verification. Callers must not discard that scope or reconstruct
paths. SI06 removes the remaining identity representation parameters, optional-table
shared helpers and structural no-op ID path predicates once their other users
migrate. The current encoder still maps owned wire records and can copy strings
before serialized-size checks; bounded borrowing wire conversion remains part of
the planned codec-private model, not a claimed memory/performance improvement.

Validation: `cargo check -p kagari-bytecode --lib`,
`cargo build -p kagari-bytecode --lib` and library Clippy with `-D warnings` passed.
The previous 159 bytecode errors are resolved. Moved six existing artifact scope,
seal, size, malformed-envelope, Host path and Host fingerprint tests into
`crates/kagari-bytecode/tests/identity.rs`; no original test is duplicated. Their
assertions now cover retained snapshot prefixes, decoding/re-encoding, full path
record equality after explicit import, stable bytes/fingerprints after a foreign
context import and reversed Host insertion order, rejection against a foreign
scope, and a forged current-format file-local reference. All six passed using a
direct test harness linked to the workspace's freshly built default-dev libraries.
Reproduction: build the bytecode library with `--message-format=json`, take the
`.rlib` filenames for `bincode`, `kagari_bytecode`, `kagari_common` and `kagari_types`
from its `compiler-artifact` records, compile the tracked integration source with
`rustc --edition=2024 --test crates/kagari-bytecode/tests/identity.rs -L dependency=target/debug/deps`
and the corresponding four `--extern name=filename` arguments, and run the produced
binary under `target/`. This checks the real integration source with no disabled
assertions or alternate production implementation; it does not establish Cargo
integration acceptance.

`cargo test -p kagari-bytecode --test identity --no-run` is blocked by 59 runtime
library errors through the existing `kagari-stdlib` dev dependency. Representative
sites: `objects/calls.rs:222` still calls the removed `trait_contract_in`;
`cache.rs:41` attempts to serialize process IDs directly;
`host/registry.rs:57` validates a path Host record;
`module.rs:153` expects the old path-returning editable extraction;
`native/interfaces/binding.rs:139` still supplies path call contracts;
`reload.rs:108` combines path and ID module records. These are SI02 registration
and SI05 runtime adoption work, and own the next repairs. Other bytecode unit
fixtures, MIR/compiler/backend consumers and source-free/pinned reload tests remain
unmigrated/unrun; the previously carried HIR adapter failure is not cleared. Do not
restore path semantic entrypoints or claim full SI04/SI05 acceptance from the library
and direct harness results. The exhaustive feature/backend matrix remains CI-only.

The directly affected array repetition/cancellation case also passed with
`cargo test -p kagari-types --lib repetition_rejects_shared_payloads_and_checks_cancellation`.
Structural review checked changed production imports, table ownership, visibility,
path uses, representation parameters and effective LOC. Removed copied-handle
clones and an unnecessary collector visibility. The structure checker passes 1043
Rust files with no violations or exceptions; formatting and diff checks pass.


### SI05 runtime program adoption and Native interface bindings

Runtime program creation now accepts scoped editable metadata with its immutable
input table, preflights resources before import, imports into the explicit target
writer and consumes bytecode verification evidence. Loaded-module editable extraction
retains its scope. Removed `VerifiedProgram::paths` and whole-module path conversions
from dependency snapshots, interpreter invalidation and reload ABI comparison.
Those boundaries hash canonical scoped records; independent contexts are compared
by their exact portable content without confusing ID equality with generation
identity. Module-key/epoch checks, source-version Arcs, retained dependency versions,
Host relinking and publication checks remain separate and unchanged.

Raw reload preflight functions now consume the candidate and return the bytecode
or artifact verification evidence they established. Installation adopts that seal
instead of constructing and verifying the same candidate again. Artifact resource
preflight is an explicit bounded-envelope operation, also run before cross-context
import; it does not grant graph/compatibility evidence. Raw and already-verified
reload routes still check the active target and complete member ABI/path set.

A normalized runtime program must retain identities for implicit public declarations
as well as explicit type/binder references. The bytecode identity collector is now
a shared, documented owner for this concrete adoption requirement. It checks the
entire input reference prefix before registering names and returns a frozen table;
it does not establish ABI validity. Runtime normalization invokes it after explicit
ID remapping, including programs with no nominal references, and preserves the
original source-version identity. Reference-free modules still share their existing
module Arcs. This is not a second declaration resolver or a module interner.

Native interface member lookup now borrows scoped contracts from the pinned program,
queries method IDs from that snapshot and carries an ID plus its retained owner.
The public member handle exposes its declaration ID and matching table; explicit
path-taking queries resolve at entry. Closed and generic interface signatures use
scoped checked call evidence directly, without catalog path export/reimport. Existing
public/ambiguous-inheritance rules, generic call witness keys, type provenance,
program leases and cache version checks remain. Native registry link validation also
uses the installed scoped import directly. Four runtime trait lookup consumers now
call the fixed-ID contract owner.

`cargo check -p kagari-runtime --lib` now reaches 20 errors, all in unmigrated Host
registration/reflection code: `host/registry.rs`, `host/path_fingerprint.rs`,
`host.rs` and `metadata/host.rs`. This supersedes the 59-error runtime diagnostic.
Temporary migration mistakes in the trait-call argument order and borrowing of
interface contracts from temporary loaded handles were repaired; scoped interfaces
now borrow the immutable program module array. Runtime library checking and runtime
behavioral tests are still not passed. SI02 Host input adoption must own the remaining
path records and registration context, including member/path contracts and reflection
fingerprints; do not restore path validators to resolve these errors.

Bytecode library build and Clippy pass after sharing the collector. The two affected
tracked identity tests (consuming seal/extraction and imported Host path/canonical
encoding) pass again through the direct harness described above. Earlier unchanged
focused checks remain evidence for their own code; no workspace sweep was run.
Structure passes 1043 Rust files with no exceptions; changed production imports,
visibility, borrowed ownership and retained version checks were reviewed. Formatting
and diff checks pass. Runtime pinned-reload/interface behavior, the normal Cargo
integration route, remaining bytecode fixtures and downstream MIR/compiler/HIR
migration remain required, with the exhaustive feature/backend matrix reserved for CI.

### SI02 Host registry adoption and reflection

Host function/type registration now accepts scoped ID declarations. Runtime creation
shares the Native catalog's definition writer with the Host registry; inputs are
explicitly imported before semantic validation and installation. Installed function,
type, member, reflection and path records retain IDs, with the registry or retained
function input owning the matching table. Host interface export returns scoped
metadata, sorts by canonical identities and does not materialize paths. Foreign
interface linking validates the original snapshot before import; normalized module
linking requires the runtime context and uses declaration IDs directly.

Removed the duplicate `HostPathSegmentRegistration` model. Descriptor registration
now combines the existing scoped semantic segment records with runtime-local physical
endpoint types. Field membership/visibility, access, dynamic parameter, result type,
schema and canonical fingerprint checks remain. The semantic path registration route
imports once; the descriptor route validates an already-local scope without another
identity remap. Function registration similarly consumes local records directly.
Host type reflection still builds and validates a candidate batch, including mutual
references, before publishing. Identity interning on failure grants no declaration
or runtime binding. Callback value validation consumes the shared IDs and preserves
heap ownership, borrows and generation-pinned nominal evidence.

Removed the unused `Runtime::host_mut` accessor: replacing the complete registry
would sever its shared identity ownership. Existing Runtime registration methods
remain the mutation boundary; no repository callers depended on the accessor.
This is part of the ID ownership change, not SA21 registration unification.

Validation for this boundary:

- `cargo check -p kagari-runtime --lib` passes, resolving the previously recorded
  20 Host registration/reflection errors. Runtime library build and scoped Clippy
  (`cargo clippy -p kagari-runtime --lib -- -D warnings`) pass. Four needless
  borrows reported by the first Clippy run were repaired without allowances.
- `cargo test -p kagari-bytecode --test identity --no-fail-fast` now passes all six
  tracked tests through Cargo, superseding the earlier dev-dependency blockage.
- Migrated the existing `host_registration`, `offline_host`, `host_nominal` and
  `offline_types` integration fixtures to explicit scoped authoring inputs. Their
  26 tests pass against the Cargo-built runtime library. These cover callbacks,
  scoped borrows, independent-table linkage, retained-prefix/foreign-ID rejection,
  exact nominal ownership, cross-runtime roots/views, path contracts/fingerprints,
  batch atomicity and mutual references, Host trait bindings and publication/reload
  checks. No assertions or callbacks were disabled and no parallel semantic fixture
  implementation was introduced. A retained-prefix case extends the existing offline
  link test; a duplicate registration uses already-local metadata. Affected cases
  were rerun after the local-adoption change.
- The attempted normal command `cargo test -p kagari-runtime --test
  host_registration --test offline_host --no-fail-fast` is blocked before running
  tests by 10 HIR and 109 MIR errors in the existing migration. Representative HIR
  owners are `aggregates/protocols.rs`, `host.rs` and `typeck/body/host_access.rs`;
  MIR verification and application queries still supply path records to scoped
  contracts. SI03/SI04 own these carried errors; do not restore path validators.
  Runtime integration tests have not passed through Cargo yet.
- The direct test method uses `cargo build -p kagari-runtime --lib
  --message-format=json`, selects the exact reported `.rlib` files for
  `kagari_runtime`, `kagari_bytecode`, `kagari_common` and `kagari_types`, then invokes
  `rustc --edition=2024 --test` on each tracked integration source with those
  `--extern` files and `-L dependency=target/debug/deps`. Execute the resulting test
  binary. This uses the workspace default development profile, target and
  parallelism; it is focused behavioral evidence, not Cargo or CI acceptance.

Changed production imports, visibility, scope ownership and publication order were
reviewed. Structure checks pass 1043 Rust files with no violations or exceptions;
formatting and diff checks pass. Remaining Host call-site fixtures/examples and the
source/compiler consumers must migrate with their owners. Native builder/catalog
adoption, HIR and specialization, codec representation removal and final documentation
cleanup remain required. No full workspace or backend matrix was run; exhaustive
validation remains CI scope. No performance or retained-memory improvement is claimed.

### SI02 scoped ModuleDecl and Native module/catalog boundary

Replaced the path-only `ModuleDecl` algorithms and changed ModuleDecl/ImplDecl's
semantic defaults to DefinitionId. Native declaration enumeration, structural/type
validation, receiver ownership, exports, callable requirements, supported-type checks,
trait implementation checking and generic normalization now borrow scoped IDs and
an explicit table. Removed the temporary `module/adoption.rs` bridge. Construction
collects implicit names through `collect_definitions`; it rejects oversized module
counts and the complete foreign/absent input reference set before interning any name.
Identity lookup helpers are read-only and fallible. They do not mint identities or
establish declaration existence by validation side effect.

Split implementation construction/checking into its ordinary owner module. Both use
one scoped method projection; checking no longer clones a complete ModuleDecl and
reenters its authoring API. Associated outputs, method binder owners/positions,
default omission/override policy, exact method signatures and local supertrait
requirements remain checked. `GenericBound::normalize` merges equal targets and
constraints and orders their types by the existing exact canonical comparator.
Fallible merge sorting propagates cancellation/invalid scopes instead of using slot
order or swallowing comparison errors. This replaces numeric-ID ordering when
combining implementation and method bounds; it is shared groundwork for the remaining
Native builder normalizer, whose old path-only code must still be removed.

Native catalog provider construction now accepts scoped metadata, imports once into
its context and validates fixed-ID modules. Receiver and implementation queries
borrow catalog records directly. Catalog documentation is keyed by ID, explicitly
remapped on import, and remains excluded from executable contract equality. Imports
and merges collect implicit trait/implementation method and enum variant names:
these may have no explicit references, so importing the reference closure alone is
insufficient. Record insertion continues to check the complete source reference
scope. These names confer no semantic authority.

NativeModule finalization now consumes records, binding keys and storage keys in its
provider context. Exact exports, storage admission, receiver ownership, dependency
closure and binding grouping use IDs; export dependencies no longer project their
contracts to paths. Installation imports the retained module declaration before
implementation checks. `to_declaration` returns editable scoped metadata rather than
a whole-record path projection, and `trait_id` returns an ID in the module's retained
table. Builder construction/typed authoring handles must now supply these scopes;
this is an intermediate architecture checkpoint, not completed Native integration.

Validation and carried errors:

- `cargo check -p kagari-types --lib` and `cargo clippy -p kagari-types --lib -- -D
  warnings` pass. Initial needless-borrow diagnostics were repaired. The expanded
  fixture initially shadowed a ModuleIdentity with ModuleDecl; that compile error
  was repaired before the selected tests completed.
- `cargo test -p kagari-types --lib declaration:: --no-fail-fast` passes 11 selected
  declaration/Host member tests. Existing canonical-order and scoped-trait fixtures
  now also check bound merging across reversed ID allocation, duplicate constraints,
  cancellation/foreign scope rejection, Native template enumeration, associated-output
  implementation construction, changed method rejection and stale-prefix rejection.
  The affected trait fixture passes again after adding the declaration-count preflight,
  including an oversized module that must not intern names into an empty context.
- `cargo check -p kagari-runtime --lib --message-format=short` initially reported 151
  errors after the shared API replacement; after catalog/module migration it reports
  126, concentrated in Native builder/type-handle and registration callers. Examples
  are `builder/enum_builder.rs` still supplying path IDs to lookup helpers,
  `builder/implementation.rs` still building path receivers, and
  `registration.rs`/`future/registration.rs` still using old declaration admission.
  No errors are reported in the migrated catalog or NativeModule files, but the
  runtime library as a whole is broken. SI02's builder/provider-handle and registration
  follow-up owns these errors. Do not add path validators or compatibility entrypoints.
- The prior Host/bytecode test results remain evidence for that earlier checkpoint;
  their Cargo routes are currently blocked again by this Native integration work.
  Native runtime behavior and source-free registration have not passed for this
  checkpoint. HIR/MIR carried errors, other fixtures and exhaustive CI remain pending.

Changed production import scopes, fallible ownership queries, public metadata
extraction and implicit-name collection were reviewed. Structure passes 1045 Rust
files with no violations/exceptions; formatting and diff checks pass. No workspace
sweep or performance measurement was run. The remaining catalog `paths`/`scope`
helpers serve unmigrated typed authoring handles and must disappear with those callers;
this checkpoint does not satisfy SI02 or SI06 exit conditions.

### SI02 scoped Native construction and typed handles

Native builders now own a construction context imported from their provider catalog.
Function, type, enum, trait and implementation construction uses DefinitionId records;
typed inputs carry a retained root snapshot and are explicitly adopted before entering
that context. Composite `Type` values carry no tables in nested nodes. Identity-free
scalar/composite inputs need no table allocation; the optional root table denotes this
absence of references, not an identity-representation choice. Combining foreign input
roots imports them into one explicit context. Retained prefixes are validated before
acceptance. Type/trait handles also collect implicit variant/method identities when
adopted, including members not present in the record's reference closure.

Removed the Native catalog's unused whole-record `paths`/`scope` bridges and old
path-key lookup helper. `FunctionDecl` retains a small typed input buffer until lowering;
it is not a second declaration model. Signature setters, applied traits, constructors
and requirement tokens now expose fallible adoption where needed. Bound normalization
uses the shared scoped normalizer. Inherent method output is sorted by method name,
not allocation-slot order. Each emitted implementation retains its own generic binder
and substitutes receiver types, associated outputs, bounds and method signatures.

Exact codecs now retain a scoped `Type`; opaque-object codecs retain an ID and source
snapshot. Scalar mappings return `BuiltinType`, removing the `abi_type_in<I>` adapter.
Typed and async registration validate scoped signatures before publishing declarations
or bindings. Selected-call owners compare exact borrowed identity views across retained
scopes. Enum member preparation retains its pinned layout instead of caching a projected
path; member checks compare module and complete segments. Runtime typed conversion and
opaque-payload binding adopt scoped input IDs before ordinary installed-contract,
argument, generation and payload checks. Builtin result preparation resolves the fixed
language identities in the runtime table without creating a path-shaped type tree.
Future-role selection iterates ID entries rather than projecting catalog keys to paths.

For example, two independently built `scope::one::Item` handles may have different
numeric IDs. A tuple composed from those handles imports both into one scope; its exact
codec matches a tuple of the same declared type. `scope::other::Item` remains different
and is rejected. A newer ID supplied with an older retained snapshot is rejected even
when both originated from the same writer. None of these identity operations establish
an installed declaration or executable proof.

Validation and carried work:

- `cargo check -p kagari-runtime --lib --message-format=short` passes after replacing
  the remaining Native constructor/registration callers. Runtime integration initially
  had 180 errors, then 111 and 43 as the builder/trait/implementation consumers migrated;
  those runtime-library errors are resolved.
- `cargo clippy -p kagari-runtime --lib -- -D warnings` passes after removing obsolete
  Copy-ID clones and redundant initializers. Scoped runtime library builds also pass.
- Moved 11 existing standalone registration tests from `native_builder.rs` into
  `native_registration.rs`, preserving their contract assertions. Selected-operation
  and default-method cases now author their ordinary required Hash trait locally;
  their bound/proof rule does not require the complete standard library. Renderer,
  standard-language owner and declaration-storage integration cases remain in the
  original target. Added one distinct scoped-input case for retained-prefix rejection,
  foreign IDs, cross-context equivalent identity and same-name foreign-module rejection.
- All 12 selected registration tests pass using the direct tracked-test method already
  documented above: build the runtime library with JSON artifact output, then compile
  `crates/kagari-runtime/tests/native_registration.rs` with `rustc --edition=2024 --test`,
  the exact emitted runtime/common/types rlibs and `-L dependency=target/debug/deps`.
  This covers exact binding rejection, missing entries, storage admission, independent
  impl binders, associated output/documentation inheritance, default template derivation,
  proven required operations, duplicate defaults and cross-context adoption/installation.
- The normal route `cargo test -p kagari-runtime --test native_registration
  --no-fail-fast --no-run --message-format=short` still fails before tests. It reports
  81 HIR and 109 MIR errors: HIR Native import/render code now also consumes fixed-ID
  ModuleDecl with old path assumptions (`native/api.rs`, `native/render.rs`), alongside
  carried Host/aggregate errors; MIR still supplies path records to fixed-ID contract
  and verifier APIs. SI03/SI04 own these errors. The selected tests have not passed via
  Cargo, and the remaining Native/stdlib/compiler fixtures have not been validated.

Production imports, macros, visibility, identity ordering, root scope retention,
builder ownership and publication points were reviewed. Structure passes 1046 Rust
files with no violations/exceptions; formatting and diff checks pass. Remaining Native
provider callers, unit fixtures, standard declarations, source collection/HIR,
specialization, identity-representation generic removal and final documentation cleanup
are still required. No workspace/backend sweep or performance measurement was run;
exhaustive validation remains CI scope. This checkpoint does not complete SI02–SI06.

### SI02 scoped standard-library construction and publication

The foundation inventory now builds DefinitionId types, traits, associated outputs,
functions, defaults, requirements and implementations in an explicit transient
construction context. Registration identity helpers return errors on table/resource
failure. The private RegistrationTrait classifier borrows an explicit table; removed
its identity-representation generic and optional-table adapter. Default/iterator
projection matching compares scoped associated-member IDs. Removed the separate
path-based bound-merging implementations in the default-template builders.

Partitioning still assigns the existing canonical core/alloc/std owners and per-owner
implementation occurrences. It visits the referenced ID set once, registers the target
ancestor chains in a separate publication context and remaps the complete records,
including map keys. It does not export source identities to paths. Implicit type,
variant, trait and implementation member identities are collected before read-only
publication queries. Associated constraints and generic bounds share the scoped
canonical normalizer; numeric allocation order does not become constraint order.
The extracted `Constraint::normalize` also retains cancellation and scope checks.

`catalog::declarations` and `shared` return fallible scoped metadata. StandardDeclarations
retains those immutable roots and propagates construction errors through its catalog
and declaration accessors. Native bindings receive the exact scoped declaration;
selected-call tokens retain its owner snapshot. Public String/Vec/enum lookup helpers
remain exact external identity queries, while obsolete path-shaped `identity::applied`
and `identity::enum_type` semantic constructors were removed. Standard typed collection
constructors use the runtime Type root's shared composition/adoption logic. Canonical
enum reading compares borrowed identity views while the actual layout remains pinned.

A source-free check exposed an existing documentation classification defect: enum
names were queried as AssociatedType identities. Path construction had silently made
those wrong documentation keys; scoped read-only lookup correctly rejected them.
Documentation now uses `TypeDefKind::definition_kind` for all types and checked member
queries. It does not intern missing names as a repair. Construction errors include their
owning inventory/publication step so future failures remain actionable.

Validation and remaining scope:

- `cargo build -p kagari-stdlib --lib --message-format=json` passes in the workspace
  development profile with default parallelism/target. `cargo clippy -p kagari-stdlib
  --lib -- -D warnings` passes after focused import, Copy-ID and parameter-grouping
  repairs. The implementation helper now takes the existing cohesive ImplDecl instead
  of growing a separate argument list. Machine-applicable Clippy edits were reviewed;
  final ordinary Clippy, rather than fix-mode exit status, is the acceptance check.
- `cargo test -p kagari-types --lib declaration::verify::scoped_tests:: --no-fail-fast`
  passes all four selected tests. This exercises canonical ordering, repeated-bound
  merging, invalid/cancelled scopes, complete trait parents, Native binders and changed
  implementations after the shared constraint-normalizer extraction.
- Moved the existing declaration-storage admission test from runtime `native_builder`
  into `crates/kagari-stdlib/tests/registration.rs`; its missing payload, wrong payload,
  duplicate binding and foreign-module assertions remain. Added one distinct foundation
  publication case: independently constructed catalogs have different scopes but exact
  type/trait/impl/export/entry contracts after import; the complete actual foundation
  installs independently into two runtimes and rejects duplicate installation.
- Both tracked registration tests pass through the previously documented direct-test
  method, using `cargo build -p kagari-stdlib --lib --message-format=json` and exact
  emitted runtime/common/stdlib rlibs with `rustc --edition=2024 --test` and
  `-L dependency=target/debug/deps`. Their first run failed at the wrong enum documentation
  query; the same selected cases passed after that repair. No assertions were weakened.
- Normal Cargo integration remains blocked by the carried HIR/MIR migration described
  in the preceding checkpoint. Do not rerun those unchanged failures just for another
  transcript. Source/HIR documentation-example analysis, remaining SDK consumers and
  fixtures, Native internal unit fixtures, reflection/specialization and complete CI
  feature/backend validation remain pending. The new fallible catalog and typed-input
  API callers must migrate with those owners; no compatibility bridge is retained.

Production scopes/imports, root ownership, fallible identity registration, complete
remapping, canonical ordering, publication points and changed Rust file sizes were
reviewed. Structure passes 1048 Rust files without violations/exceptions; formatting
and diff checks pass. No full workspace suite, performance measurement, ABI/version
bump or generated-fixture sweep was run. SI02 still includes source registration and
remaining call-site integration; SI03–SI06 and final documentation cleanup remain open.

### SI02 scoped Native declaration views

The HIR-owned renderer now accepts scoped ModuleDecl metadata and provider roots.
It borrows definition names, parents and complete paths from the retained table;
its generated navigation sites use DefinitionId and retain that exact immutable
prefix. Rendering does not intern names or grant registration authority. Missing
implicit declaration names return errors instead of fabricating new path identities.
Provider package spelling reads only provider records and does not combine their
independent identity scopes. Associated-output rendering sorts by complete borrowed
identity, preserving canonical text independently of numeric allocation order.

ModuleDecl construction now also collects implementation-owned associated output
names. These are the generated `impl { type Output = ...; }` declaration identities,
not replacements for the trait-owned associated keys in semantic applications.
Registration remains fallible and first validates all supplied references against
the incoming prefix. The Native array-interface lookup now takes an explicit table,
indexes actual type/trait declarations by ID and rejects duplicate identities;
receiver and interface matching no longer reconstructs paths.

Validation and carried integration:

- `cargo check -p kagari-hir --lib --message-format=short` was attempted at the
  changed owner boundary. It reports 37 errors, down from the preceding 81-error
  HIR attempt. No errors remain in the renderer or array-interface helper. The
  remaining diagnostics are in `native/api.rs`, Host consumers and aggregate
  protocols: old unscoped inputs, path-shaped lowered attachments and obsolete
  Host validation/accessors. SI02 source/provider adoption and SI03 own those
  repairs; HIR does not build yet. The previously recorded MIR failures remain.
- `cargo build -p kagari-stdlib -p kagari-syntax --lib --message-format=json` passes.
  The two tracked tests in `native/render/tests.rs` pass through a temporary direct
  Rust test harness that compiles the actual `native/paths.rs` and `native/render.rs`
  modules against Cargo's exact emitted common/types/stdlib/source/syntax rlibs.
  Use an inline `native` module whose `#[path]` is the repository's native directory,
  then ordinary `mod paths; mod render;`; no copied production implementation or
  tracked module-path exception is involved. The same harness passes direct
  `clippy-driver --test --emit=metadata -D warnings` with those exact rlibs.
- Those tests compare all generated foundation text and declaration/name/generic/
  parameter/bound spans across independent scopes, parse every generated module,
  check both registered array interfaces and reject foreign tables, retain old
  navigation prefixes after appends, and reject missing implicit names. The
  canonical comparison exports individual site identities only at the test boundary.
- Both existing stdlib registration tests pass again against the newly built rlibs,
  covering the changed implicit-name collector through complete foundation admission,
  independent runtime installs, duplicate rejection and local storage contracts.
  `cargo clippy -p kagari-types --lib -- -D warnings` also passes.

Production imports, table retention, fallible queries, generated-source authority,
identity ordering and file sizes were reviewed. Structure passes 1049 Rust files
without violations/exceptions; formatting and diff checks pass. Normal Cargo HIR
unit/integration execution and source correspondence/recovery tests remain pending
until the owning migration builds; direct harness results do not claim those checks
passed. No full workspace/backend sweep or speed claim was made. Continue with
Native input adoption into the analysis context, source identity collection and
fixed-ID HIR consumers; SI02–SI06 remain incomplete.

### SI02 Native inputs adopted into the analysis owner

AnalysisDatabase and the single-source entrypoint now accept explicit scoped provider
metadata. Before preparing installed sources, the declaration query imports the complete
provider batch into the database's existing DefinitionContext, collects implicit names,
and retains one completed prefix for every module. This happens once per native-files
preparation, not separately for each rendered module. Duplicate logical provider modules
and cancellation fail without publishing the native-files cache; harmless context appends
do not establish declaration or executable authority. Replacing inputs still invalidates
the existing declaration/signature/body/file caches, and database clones still share the
append-only identity context.

Native import now validates the scoped ModuleDecl directly against receiver owners and
the adopted table. It preserves exact non-trivia source correspondence before attaching
registration facts. LoweredModule stores fixed-ID trait contracts, native entries/defaults,
storage descriptors, array bridges and conversion adapters, with one retained registration
prefix. Its public declaration accessor exposes IDs and a matching table accessor. Ordinary
source lowering has no Native registration prefix. Dependency discovery borrows complete
identity views; removed the per-receiver `scope_record` path conversion. Package spellings
remain presentation queries. Provider iteration borrows records instead of cloning their
complete contracts to satisfy the array-interface helper.

Focused evidence and remaining integration:

- `cargo check -p kagari-hir --lib --message-format=short` reports 21 errors, reduced
  from 37. Native adoption/import/render and the changed database input signatures
  type-check. The remaining errors are fixed-ID lowered records reaching path-shaped
  declarations, import bridges, trait adapters, role validation and type checking,
  plus carried Host/protocol consumers. SI02 source identity collection and SI03 own
  these coupled repairs; the HIR library still does not build.
- The two existing renderer tests and one batch-adoption test pass through the same
  direct tracked-module harness documented above, now including `native/adoption.rs`.
  Mixed providers from two independently constructed foundation contexts normalize
  into one analysis scope, preserve rendered text, supply both array contracts, and
  retain identical referenced IDs on repeated adoption. Duplicate providers, foreign
  array tables and cancelled empty/nonempty batches reject; retained prefixes reject
  later additions. The harness also passes direct Clippy with warnings denied.
- Reviewed unchanged Native source correspondence logic and cache publication/cancellation
  ordering. Full source correspondence, query reuse/recovery and normal Cargo HIR tests
  remain unrun because their owners do not yet build. Previous source-free runtime,
  stdlib and type checks remain evidence for unchanged code; no workspace sweep was run.

Structure passes 1050 Rust files without violations/exceptions; formatting and diff
checks pass. The next coupled boundary is source declaration collection: its current
Builder constructs paths, then signature/body checking and cache publication project
between paths and IDs. Migration must also preserve the existing oversized-module
recovery test: invalid source identity yields a CompileLimitExceeded diagnostic and
cannot enter code generation, rather than fabricating a valid definition identity or
panicking while interning. Source/HIR migration, specialization integration, obsolete
representation removal and final documentation cleanup remain required.

### SI02/SI03 source registration and semantic representation cutover

Source Declarations now registers the real module root, headers and members directly
in the analysis DefinitionContext before signature checking. Duplicate disambiguation
uses `(parent ID, kind, name)` occurrence counters; generic binders keep their declaring
owner and position, including inherited method parameters. Root/member registration,
occurrence exhaustion and cancellation return mapping errors without publishing a
partial declaration result. Local BindingId construction reuses the collected body ID
and no longer interns its path or extends the retained prefix.

HIR semantic model defaults now select DefinitionId: declarations, types/substitutions,
Native descriptors, signatures, type tables, call/propagation facts, imported contracts
and aggregate records. The remaining identity parameters and mapping implementations
are transitional SI06 deletion targets, not a second path-based checking engine.
The signature/type-table indexes and array bridge facts use ID keys directly. Shared
semantic types raised into HIR retain their existing IDs. Language-role comparison
uses exact table lookup and the shared canonical GenericBound normalizer rather than
numeric ID ordering.

Removed the analysis-wide `ownership::paths` helper and declaration/signature authoring
copy APIs. Signature catalogs, body reuse comparisons, completion, call queries and
position type queries now borrow retained scoped facts or copy only the records they
need. Source revision, imports, namespace surfaces, host revision, environment contracts
and cancellation remain reuse dependencies; ID equality does not replace them. Cache
publication validates fixed-ID records against the analysis prefix instead of interning
paths. Checked/file editable extraction retains DefinitionMetadata and drops executable
evidence; removed the duplicate path-shaped code-generation entrypoint.

Unrepresentable source module identities retain their existing limit diagnostic, parsed
HIR and lexical resolution. Declaration recovery creates no invented IDs; signature,
aggregate and body boundaries leave semantic facts empty for that diagnosed module.
The standalone recovery path still uses the normal analysis pipeline. This also avoids
panicking on inline over-limit modules reaching query preparation. The existing
`oversized_source_module_identities_are_rejected_without_panicking` test remains the
acceptance owner, together with identity/navigation/reuse tests; they have not yet run
against this cutover because the HIR crate is not buildable.

Intermediate validation and owned follow-up:

- `cargo check -p kagari-hir --lib --message-format=short` was attempted after the
  representation change and related repairs. The first attempt exposed 398 errors;
  the final attempt reports 325 errors and no warnings. There are no diagnostics in
  source declaration collection, the changed scoped cache preparation, role validation
  or the recovery orchestration. This is compiler diagnostic evidence, not a passing
  HIR build or successful behavioral validation.
- Remaining errors are concentrated in `language/semantics.rs` (34), aggregate
  implementations (33), associated-type checking (30), Host inputs (28), type resolution,
  member/constructor checking and diagnostics. Representative failures access `.path`
  or `.module` on an ID or pass path-shaped Host/protocol data to fixed-ID consumers.
  SI03 owns explicit table access/adoption for these mechanisms. Aggregate/name queries
  must obtain the enclosing catalog/analysis scope, not attach a table to every type
  node or restore whole-record path projections. Source/Native fixture and SDK/compiler
  call sites also need the new scoped extraction APIs. Carried MIR errors remain SI04.
- Numeric-ID-keyed traversal still needs the planned semantic-order audit as these
  consumers migrate; successful compilation alone is insufficient. The fixed-ID
  declaration visitor/remapper must include BindingId's body owner when obsolete
  arbitrary representation mapping is removed; the old generic visitor skipped that
  already-scoped field. Preserve its analysis/arena qualification as well.
- Normal HIR tests and Clippy cannot run successfully before these owners compile.
  Do not count unexecuted identity, alias/navigation, rejection/recovery or reuse cases
  as passed. Prior Native rendering/adoption and source-free runtime checks remain
  evidence for unchanged owners; no unrelated suite was rerun for a green transcript.

Changed handwritten Rust imports, visibility, ownership, publication points and sizes
were reviewed. Structure passes 1050 Rust files without violations/exceptions; formatting
and diff checks pass. This is an explicitly broken intermediate architecture checkpoint,
not SI02/SI03 acceptance. Continue the fixed-ID HIR consumers, then specialization and
codec integration, representation-generic deletion and final documentation cleanup.

### SI03 scoped aggregate members and associated projections

Declaration collection now registers every associated constant header, including
required constants with no initializer. A header and its optional initializer share
one identity; reverse lookup retains the initializer's executable HIR handle. Header
navigation uses the collected source-origin metadata. Associated type parameters,
bounds and implementation families likewise consume collected member identities,
preserving occurrence disambiguation instead of reconstructing member paths.

Associated projection resolution and normalization use IDs. Named projections look
up an existing child in the enclosing declaration table after checking the actual
trait surface and arity; an undeclared member remains an error, not a newly interned
definition. Imported parent-surface refinement and signature preparation refresh their
working declaration prefix after all source declarations have been collected, so a
forward/imported member can resolve without mutating retained earlier snapshots.

Omitted default methods required a separate registration point: previously aggregate
selection fabricated an impl-owned path from the trait method's final segment. After
signature checking resolves the implemented trait, `definition_registration` reserves
its declared method identities under that implementation, retaining kind/name/occurrence.
Both local and imported trait surfaces participate. This only assigns identity; default
selection still checks the actual method contract and explicit overrides. Cancellation
or registration exhaustion prevents publication; prepared signatures retain the new
prefix. Reused signatures follow the same idempotent registration step. Aggregate
method selection consumes these registered IDs through an explicit table; it never
interns a method during selection or silently drops a valid default on missing identity.

Aggregate module filtering now consults the supplied table's module ownership and
scans each owning map once, replacing path ranges that do not work on numeric IDs.
No parallel module interner, per-type table or representation-dependent optional scope
was introduced. Associated families use table parent/child queries to relate impl
outputs to declared trait outputs. Trait closure and concrete implementation results
carry IDs. Overlap diagnostics sort by the table's exact identity comparison before
selecting the first conflict, preserving canonical conflict order independently of
allocation order. The broader audit of catalog traversal, diagnostic/display ordering
and specialization/artifact order remains required under SI03/SI04/SI06.

Intermediate validation and remaining scope:

- `cargo check -p kagari-hir --lib --message-format=short` was attempted at the changed
  boundaries. The final check still fails with 236 errors and no warnings, down from
  the previous checkpoint's 325. The new registration stage, declaration collection,
  associated projection module, imported surface cache and aggregate trait collection
  have no compiler diagnostics. This is not a successful HIR build.
- Remaining errors include path-shaped Host inputs, language protocol constructors and
  identity classification, type display, aggregate normalization and affected body
  checking. These remain owned by SI03. Explicit table arguments to aggregate default
  queries also require the planned compiler/specialization and fixture migration;
  no compiler or SDK acceptance is claimed. The carried MIR boundary remains SI04.
- Once HIR builds, select existing `trait_catalog_tests`, `trait_identity_tests`,
  `associated_const_tests`, associated-family/projection contracts and oversized-source
  recovery coverage. They own defaults/override identity, cross-module isolation,
  navigation/rebasing and recovery. Normal HIR behavioral tests and Clippy remain
  unrun while the library fails to compile. No duplicate smoke tests or workspace
  sweep were added; prior tests of unchanged Native/runtime owners were not rerun.

Structural review covers new imports, the explicit registration owner, fallible
publication and table-prefix use. Structure passes 1051 Rust files without violations or documented exceptions;
formatting and diff checks pass. This remains an
explicitly broken intermediate migration checkpoint, not phase acceptance. Continue
Host/protocol/display consumers, then the remaining specialization/codecs and removal
of identity representation parameters before retiring the plan.

### SI03 scoped Host analysis inputs and queries

HostInput now accepts DefinitionMetadata of a fixed-ID HostInterface, with origin
keys in that same retained scope. HostDeclarations validates the interface against
its table, expands declared methods through the existing checked method-contract
API, and retains fixed-ID contracts, signatures and indexes. Origins still require
an actual declared function/type/field (including expanded methods) and a valid
preferred location; an interned ID alone supplies no declaration or binding authority.
Field lookup uses the table parent relation; method classification and path-contract
validation use the retained table rather than projecting an entire contract to paths.

The first declaration query adopts the complete Host interface into the database's
existing context. A single ancestor-closure import remaps contracts, origin keys,
method-owner indexes and nominal-type indexes. The immutable Host revision and
function/type/module slot numbers stay unchanged: these identify the original input,
independently of its adopted definition IDs. Semantic signature types are derived from
those adopted contracts. The database caches this adopted input until
`set_host_declarations` replaces the source input; this avoids repeating adoption for
every file/query. Cancellation or mapping failure publishes no adopted input. Existing
source, import, namespace and Host revision comparisons continue to control query reuse;
old file snapshots retain their prior input and definition prefix.

File and signature Host navigation queries now return scoped contracts directly.
Removed the remaining field-query ID-to-path projection. Host argument/field/path
checking consumes the same adopted ID types, retaining declared access, dynamic index
contracts and revision-qualified callable selection. No runtime callbacks, Rust host
state or borrow authority moved into HIR. The public empty-Host convenience now reports
identity-allocation failure; single-module name resolution returns AnalysisError so
this failure is propagated alongside import failures. The existing infallible
AnalysisDatabase default constructor retains its explicit allocation expectations.

Intermediate evidence and follow-up:

- `cargo check -p kagari-hir --lib --message-format=short` still fails with 185 errors
  and no warnings, down from 236 at the previous checkpoint. Host input/adoption,
  source/name preparation, callable projection and Host navigation modules have no
  compiler diagnostics. This does not establish behavioral acceptance.
- The remaining Host call-checking errors are the `print` helper consuming the old
  path-shaped `standard_log` authoring constructor. Migrate that shared contract at
  its declaration owner and update its consumers; do not add a second HIR-only log
  signature or a temporary whole-record path projection. Protocol/type-display and
  the other carried semantic errors remain SI03. Source SDK/Host fixtures still need
  scoped construction APIs; full consumer integration is not yet buildable.
- Existing Host facade/origin/type tests own stale-slot rejection, snapshot-owned
  origins, interface implementation checks and declared field/index access. Their
  fixtures must move to scoped inputs before these selected tests run after HIR
  builds. Normal HIR behavioral tests and Clippy remain unrun; no workspace sweep or
  unrelated passing suite was rerun. Runtime borrow/source-free evidence from earlier
  unchanged owners remains separate from this unverified source-analysis cutover.

Reviewed scope ownership, ID-key remapping, revision preservation, cache lifetime,
fallible empty-scope construction and cancellation before publication. Structure
passes 1052 Rust files with no violations or documented exceptions; formatting and
`git diff --check` pass. This is a broken intermediate SI03 checkpoint. Remaining
shared constructors/protocols, semantic consumers, specialization/codecs, obsolete
representation removal and final documentation cleanup are still required.

### SI03 shared scoped log contract

The shared `standard_log` constructor now returns fixed-ID DefinitionMetadata. It
registers the existing exact `host` package / `host` module / `log` function identity
directly in its construction context. One immutable contract owner retains that scope;
repeated calls clone its metadata rather than allocate new identity universes or build
path-shaped function records. This is a fixed built-in declaration, not a global pool
for arbitrary identities. Its lifetime and validity are process-wide and independent
of user providers; installed runtime contracts still pass normal adoption and binding
validation. The symbol, borrowed String parameter, Unit result and effects are unchanged.

Source declaration collection initializes this fallible contract before publishing
semantic facts. Print checking borrows its scoped signature and no longer invokes the
obsolete path-based Host function constructor or method-owner projection. No log ID
escapes into another scope through HIR print facts: the existing RuntimeHelper selection
remains the representation at that point. Compiler native-import construction must
still adopt the contract into the program scope during SI04. CLI registration now
propagates construction failure; direct runtime/example registration consumers take the
scoped metadata. The two VM analysis helpers now construct scoped HostInterface inputs.
Other already-carried fixture/API migrations are not claimed complete.

Focused evidence:

- `cargo test -p kagari-types --test host_identity
  standard_log_retains_scoped_contract_and_canonical_binding_after_adoption -- --exact`
  passes. It checks the exact canonical identity and shared signature/effects, repeated
  retained scope, explicit foreign-context import, old-prefix rejection and invariant
  binding fingerprints after import.
- `cargo test -p kagari-bytecode --test identity
  required_host_fingerprint_is_derived_and_independent_of_docs_and_order -- --exact`
  passes after migrating this existing fixture to scoped construction. It preserves
  the independent-context/reversed-allocation case, documentation/order invariance,
  effect sensitivity and artifact fingerprint checks. The first attempt exposed a
  stale reference to the removed authoring fixture; that test-only error was repaired
  and the selected case rerun successfully.
- `cargo clippy -p kagari-types --lib --test host_identity -- -D warnings` passes.
- `cargo check -p kagari-hir --lib --message-format=short` still fails with 181 errors
  and no warnings. The four carried print/Host-value errors are gone; protocol,
  aggregate/type-display and other semantic consumers remain SI03. Normal HIR,
  compiler, CLI and VM behavioral acceptance remains pending. Updated registration
  call sites in those dependents have not been covered by a successful dependent build.

Structure passes 1052 Rust files with no violations or documented exceptions; formatting
and diff checks pass. No ABI/format bump or workspace sweep was performed. Continue the
protocol scope migration and other carried HIR errors, then the required program/codecs,
identity-generic deletion, selected integration checks and final documentation cleanup.

### SI03 scoped protocol semantics and implementation queries

Protocol lookup now resolves an existing canonical module root and trait child in
an explicit DefinitionTable. The lookup does not intern identities or confer a
language role. HIR protocol views, associated outputs, dynamic iterator erasure,
projection normalization, bounded implementation searches, member queries and
TypeTable semantic queries retain DefinitionId and accept their owning table.
Implicit native protocol applications now carry fixed-ID requirements/signatures.
Body checking threads the analysis snapshot through operator, iteration, callable
and propagation selection; ControlFlow and associated members are looked up rather
than constructed as path-shaped semantic records. Host validation uses the complete
analysis table because its original adopted input prefix predates source identities.

The distinction between intrinsic representation rules and declared nominal views
is preserved: scalar/tuple equality and hashing still work without a registered
protocol identity, while producing a nominal protocol/member requires it to exist
in the supplied prefix. Searches retain candidate/depth limits, cancellation and
cycle/default guards. Review caught and repaired a shadowed receiver variable in
standard trait closure comparison; the comparison targets the required protocol.
No global identity pool, stored per-node table, alternate semantic engine or format
version change was introduced. Shared authoring path helpers and representation
parameters still require removal/consolidation under the existing SI06 scope.

Focused evidence:

- `cargo test -p kagari-types --test language_identity -- --exact
  protocol_lookup_requires_exact_identity_in_the_retained_scope` passes. This checks
  all canonical protocol identities, same-name/wrong-kind/duplicate-occurrence and
  foreign-package rejection, immutable retained-prefix visibility, and explicit
  foreign-context import without accepting the source handle in the target scope.
  The first attempt used the wrong test-only remap method name; repaired to `map`
  and rerun successfully.
- `cargo clippy -p kagari-types --lib --test language_identity -- -D warnings` passes.
- `cargo check -p kagari-hir --lib --message-format=short` remains an intermediate
  failure with 43 errors and no warnings (previous checkpoint: 181). The migrated
  protocol/aggregate/member/TypeTable algorithms have no compiler diagnostics.
  Remaining errors are associated constants, constructor/member/field consumers,
  associated-family diagnostics and TypeId display still using path fields or
  explicit path types. These remain owned by SI03, followed by downstream SI04
  integration; this count is not behavioral acceptance.
- HIR protocol/equality/iteration/propagation behavioral tests remain pending until
  these carried library errors and their scoped fixtures are repaired. Select the
  existing contract cases for those mechanisms and missing-provider/recovery before
  claiming acceptance; do not substitute the shared identity test for execution
  behavior. Compiler consumers of changed query signatures remain pending SI04.

Structure passes 1053 Rust files with no violations or documented exceptions;
formatting and diff checks pass. Manual review covers explicit scope ownership,
imports, associated member lookup, host-prefix
lifetime and retained search guards. No broader Cargo test sweep was performed;
exhaustive feature/backend acceptance remains CI scope. This is a broken intermediate
SI03 checkpoint, not completion of SI03 or SI01-SI06.

### SI03 member consumers, scoped diagnostics and build recovery

HIR's production library now builds with fixed-ID semantic consumers. Constructor,
field/place/pattern and associated-constant selection no longer return DefinitionPath.
Visibility checks resolve borrowed owner modules in the analysis table, and associated
constants look up the registered Const child before consulting the trait contract.
Family/member diagnostics borrow names from the same scope. No name-only fallback
supplies a semantic identity, and access checks and source diagnostics remain in place.

TypeId diagnostic formatting now takes an explicit retained DefinitionTable. Its
implementation lives in `types/display.rs`; callers pass their declaration/analysis
scope, including Host contract validation and constant checking. Formatting uses
borrowed name segments and orders associated members by canonical identity rather
than numeric ID allocation. It remains a diagnostic spelling, not a portable ID or
source parser. Absent/foreign named identities are invariant violations; retained
snapshots keep their names after the writer grows. No per-type table or path-shaped
semantic copy is introduced.

Removed obsolete clones of Copy IDs in production HIR with compiler-guided edits.
Application validation now groups its checked catalog, Host contracts, TypeTable and
definition table in ApplicationContext. Constant validation similarly groups immutable
checked inputs. Concrete implementation searches take ImplementationSearchLimits for
candidate/depth limits; all HIR call sites retain their previous values and cancellation
argument. These changes keep scope and resource ownership explicit without new
semantic policies or lint exemptions. SI04 compiler callers must adopt the new scoped
query/display interfaces; those consumers are not claimed buildable yet.

Focused evidence:

- `cargo check -p kagari-hir --lib --message-format=short` passes, resolving the 43
  carried library diagnostics from the previous checkpoint.
- `cargo clippy -p kagari-hir --lib -- -D warnings` passes after removing 142 obsolete
  Copy clones, grouping three expanded argument lists and resolving consequent
  shorthand/lazy-evaluation findings. The first run reported 145 errors; no lint
  allowance was added. Final Clippy also checks the final production-library edits.
- Migrated the existing `language_contracts` integration fixture to scoped catalog
  metadata, existing-ID lookups and scope-aware validation. `cargo test -p kagari-hir
  --test language_contracts --no-fail-fast` passes all 10 selected contract tests.
  These cover complete portable language contracts, installed input adoption,
  concrete versus erased iterator proofs, default containers, readonly/hash-key
  rejection, Iterable item compatibility, array-only analysis without providers,
  and method documentation/navigation/completion. The added missing-provider case
  checks intrinsic scalar/tuple equality and hashing with/without an empty catalog
  while nominal protocol lookup remains absent. An initial fixture build incorrectly
  queried FileDeclarations directly for its table; it now uses its Declarations owner.
- `cargo test -p kagari-hir --test identity_display -- --exact
  diagnostic_names_preserve_canonical_order_and_snapshot_ownership` passes. It checks
  both associated-member allocation orders, exact diagnostic spelling, old-prefix
  and foreign-scope rejection, and retained names after context growth.
- `cargo test -p kagari-hir --lib
  analysis::prelude_tests::standard_variants_and_propagation_rebase_on_body_reuse
  -- --exact` cannot build the lib-test target: 536 errors remain in unmigrated test
  fixtures. Representative failures call removed `ownership::paths`, construct
  path-shaped HIR/Host records, and pass fallible catalog results to old signatures.
  The selected propagation/rebase test did not execute. SI03 owns migrating these
  fixtures and running the existing selected propagation, snapshot/identity,
  associated-constant and Host-boundary contract cases. Preserve assertions and
  compare independently scoped snapshots by explicit remapping, not path projections.

Structure passes 1055 Rust files with no violations or documented exceptions;
formatting and diff checks pass. Reviewed imports, ID copies, ownership/visibility,
display ordering, scoped inputs and unchanged search budgets. No ABI bump, workspace
sweep or full backend matrix was run. Remaining SI03 test migration and identity
visitor/remapping audits, SI04 integration, SI05/SI06 representation removal and final
documentation cleanup are still required; production HIR build recovery is not overall
plan acceptance.

### SI03 scoped test inputs and fresh/reused fact comparison

Migrated shared Native declaration fixtures to fixed-ID DefinitionMetadata. Each
fixture registers its local roots/members in an explicit construction context and
imports the exact installed Hash/Iterable/member/List identities it references.
Validation receives the retained table; generated-source correspondence tests keep
the original malformed-body/signature/visibility cases. Fallible bundled-catalog
calls now unwrap at fixture construction boundaries, including nested generic-type
test modules. Fixtures read editable/checked metadata through its records owner.

Removed every test call to the deleted `ownership::paths` projection. Same-snapshot
navigation compares scoped declarations directly. Fresh versus reused TypeTable and
ModuleSignatures comparisons import both records into one test-owned context before
comparing semantic IDs, while retaining the preexisting independent arena assertions
and local-node normalization. A test-only `import_facts` helper delegates to ordinary
metadata reference validation/import; it does not normalize analysis/arena identities
or recreate a path-shaped semantic representation. The signature navigation test's
individual type-reference comparison uses the same explicit shared-scope import.
Nominal-type and bounded-implementation-search fixtures now allocate in independent
contexts and retain their deep traversal, cancellation, owner/arity and budget checks.
The editable checked-analysis assertion now expects the retained owner ID, matching
the scoped `to_unverified` contract; separate exact-path/old-snapshot assertions remain.

Evidence and remaining work:

- `cargo check -p kagari-hir --lib --message-format=short` passes after the test-only
  helper and cfg import changes. Production behavior is unchanged by this checkpoint.
- The selected `cargo test -p kagari-hir --lib
  analysis::prelude_tests::standard_variants_and_propagation_rebase_on_body_reuse
  --message-format=short -- --exact` still fails while compiling the lib-test target:
  249 errors, no warnings (previous checkpoint: 536). The selected test has not run.
  Successive compiler attempts followed related fixture repairs; no assertions or
  test cases were removed to reduce the count.
- Remaining SI03 fixture work includes Host construction/metadata input ownership,
  source/foundation expected types still built from paths, explicit protocol query
  scope/limit arguments, mutable metadata fixture edits and old read/display APIs.
  Finish the shared Host authoring boundary at its actual owner rather than add a
  second HIR signature implementation or restore a path projection. Production Host
  adoption stays distinct from its revision-qualified callable slots.
- Preserve the earlier tracked BindingId body-owner visitor/remapper correction when
  removing representation generics; current comparisons import TypeTable/signature
  records, not local-binding identity records. Do not infer whole-analysis remapping
  acceptance from the helper's successful type checking. Audit fresh-analysis fixtures
  that share cached Native files before executing them: immutable source provenance
  and adopted definition scope must both remain valid.
- The previously passing 10 language-contract integration cases, identity-display
  case and production Clippy remain evidence for unaffected owners. They were not
  rerun merely to produce another green transcript. No new fixture runtime acceptance
  is claimed until the lib-test target builds and the selected cases actually execute.

Structure passes 1055 Rust files with no violations or documented exceptions;
formatting and diff checks pass. Reviewed test/production cfg boundaries, imports,
explicit context ownership and preservation of comparison assertions. The checkpoint
still carries the lib-test build failure; SI03 focused behavioral acceptance and the
remaining SI04-SI06 integration/removal/documentation work remain incomplete.

### SI03 scoped protocol/query and inference fixtures

Migrated Native implementation and source-query fixtures to the retained analysis
DefinitionTable. Foundation receiver/trait expectations resolve exact external
queries once; protocol/associated-member queries consume scoped IDs and the same
table as the catalog. Concrete implementation searches retain their original
4096-check/64-depth limits. Constructor, method, trait and diagnostic-name assertions
now inspect borrowed identity views rather than materialize semantic path records.
Signature tests borrow the existing prepared facts instead of the removed authoring
projection. The invalid Native default-body fixture explicitly separates and reseals
its metadata after changing a bound, retaining the table and its rejection assertions.

The installed documentation inventory resolves generated provider-site identities
separately in each independent snapshot. It still asserts that an ID from one
snapshot cannot query the other, while canonical declaration names, byte ranges,
documentation and written signatures agree. Two independent body-reuse comparison
fixtures now resolve the requested owner in the fresh database before calling its
body query; they no longer submit the older database's scoped ID. The remaining
cross-snapshot/cache audit from the preceding checkpoint is still required.

Inference fixtures register nominal and generic owners in explicit local contexts.
The collection-inference case obtains its generic function owner from ordinary
source registration in the same snapshot as the Native aggregate catalog. Deep
10,000-level traversal, cancellation, first-member inference, mismatched nominal
owners/kinds/arity and recovery assertions remain intact.

Validation and carried work:

- The selected `cargo test -p kagari-hir --lib
  analysis::prelude_tests::standard_variants_and_propagation_rebase_on_body_reuse
  --message-format=short -- --exact` still fails compiling the lib-test target:
  115 errors, no warnings, down from 249 at the preceding checkpoint. Related
  fixture batches were followed by compiler attempts; the selected test has not run.
  Remaining errors are primarily Host input/construction, old common type-checking
  helper expectations and a few callable/source metadata queries. SI03 owns their
  repair and execution of the selected cases after the target builds.
- All Rust changes in this checkpoint are test-only. Existing production HIR check,
  Clippy and focused integration results remain evidence for unaffected production
  code; no workspace suite or redundant integration rerun was performed.
- Structure passes 1055 Rust files with zero violations or documented exceptions;
  formatting and diff checks pass. Reviewed explicit table ownership, unchanged
  rejection/constraint assertions, scope of test helpers, imports and module access.
  No public API, re-export, LOC exception or ABI identifier was added or changed.

Host construction at its shared owner, the common type-checking helper's retained
scope/registration lifecycle, BindingId visitor/remapping, remaining snapshot audits,
SI04 integration and SI05/SI06 representation removal/documentation cleanup remain
required. The reduced diagnostic count is not behavioral acceptance.

### SI02/SI03 explicit Host construction context

Replaced the shared Host function/type/field/method constructors at their actual
owner with fixed-ID constructors accepting an explicit DefinitionContext. Function
and type roots use one Host namespace splitter and register children directly;
fields and methods register under the supplied owner ID, rejecting a foreign owner
through the ordinary table writer. Allocation/path failures are returned. Trait
implementation assembly now accepts scoped IDs and scoped signature records.
Removed the path-producing `HostValueType::opaque(symbol)` shortcut and the private
path-valued member factory. Callers construct `Opaque` from their registered type
ID. `host_type_identity` remains an exact external query builder, not a declaration
constructor. `standard_log` now uses the same scoped function constructor while
retaining its immutable metadata scope, signature, effects and canonical identity.
No compatibility overload or global context was introduced.

Construction still allocates identity rather than granting validation evidence.
Signature IDs are checked by metadata sealing; complete Host contracts, symbol
validity, member ownership/access and codec validation remain with their existing
owners. The focused construction case verifies foreign-parent rejection, immutable
prefixes, method self ownership, invalid-symbol validation and rejection of foreign
signature references. Host metadata integration fixtures now retain scoped inputs,
compare imported/decoded records in a common explicit context and validate decoded
contracts directly, eliminating their path-record projections. Canonical portable
bytes, cancellation, foreign references, out-of-range wire references and retained
ownership assertions remain covered.

Migrated the HIR Host input, origin and facade test owners to the shared constructors
and table-retaining metadata. Their source-navigation assertions resolve Host IDs in
the analysis snapshot after adoption; revision-qualified callable/type slots and
old-snapshot assertions are unchanged. The test-only metadata helper only seals IDs;
it deliberately leaves semantic validation to HostDeclarations so malformed input
fixtures still exercise their original rejection boundary.

Validation and carried migration errors:

- `cargo check -p kagari-hir --lib --message-format=short` passes after replacing the
  shared constructors. Production Types/HIR remain buildable.
- `cargo test -p kagari-types --test host_identity --test host_metadata --no-fail-fast`:
  six cases pass. After consolidating standard_log construction, its exact
  `standard_log_retains_scoped_contract_and_canonical_binding_after_adoption` case
  passes again. Other unaffected cases were not rerun for a new transcript.
- `cargo clippy -p kagari-types --lib --test host_identity --test host_metadata --
  -D warnings` passes; the final production-only Clippy also passes after the
  standard_log consolidation.
- `cargo test -p kagari-hir --lib
  host::tests::snapshots_own_host_declarations_and_invalidate_body_reuse_on_input_change
  --message-format=short -- --exact` cannot build the lib-test target: 141 errors,
  no warnings. Its selected test has not executed. The changed Host input/origin/
  facade modules have no reported compiler errors. The remaining Host type,
  callable/import and common source fixtures still contain old constructor/input
  APIs, alongside the previously recorded SI03 failures.
- `cargo test -p kagari-types --lib
  scoped_host_signatures_reject_foreign_and_wrong_constructor_identities
  --message-format=short` cannot build the lib-test target: 162 errors, no warnings.
  Its selected case has not executed. The shared Host path/member/value fixtures
  and scoped_tests still use removed path constructors, raw path-shaped expected
  records and the test_support projection helpers. Migrate these at the same Host
  contract owners; do not restore compatibility constructors or duplicate semantic
  factories. SI03 owns fixture recovery, coupled with SI06 removal of the remaining
  representation-generic mapping/test helpers. Downstream compiler/runtime/embedding
  fixture and example callers must also migrate in their existing owning phases.

Structure passes 1055 Rust files with zero violations or documented exceptions;
formatting and diff checks pass. Reviewed production imports, constructor ownership,
fallible allocation, metadata/semantic validation separation and preserved contract
checks. No ABI identifier bump, workspace test sweep or backend matrix was run.
The remaining unit-target failures and SI04-SI06 work prevent plan acceptance.

### SI03 Host contract fixtures without semantic path projections

Recovered the shared Types lib-test target after scoped Host construction. Host
member, path, value, decode-limit and wrong-constructor fixtures now register IDs
in explicit test-owned contexts and pass retained tables to validation,
fingerprinting, method and path-contract queries. Removed all fixture extension
traits that converted complete Host records to/from paths. The remaining small
codec helpers decode through ordinary Host validation and explicitly import into
the caller's retained context, or encode malformed semantic records with real
file references so decoder rejection can still be tested.

Preserved member/trait/associated-output ownership, duplicate declarations,
reference closure, signature/access rejection, canonical ordering, documentation
independence, schema epochs, old-version rejection, malformed sequence limits and
cross-table validation assertions. Negative fixtures register alternate identities
rather than mutate private ID contents: wrong-kind cases retain their original
module/name, and the foreign field case changes the package while retaining its
module path/member shape. Standalone Host value roundtrips now use portable
metadata and explicit import, while malformed flat type nodes use file references.
No semantic DefinitionId is serialized as an ordinary value.

The former in-memory overlong path mutation cannot be constructed under the scoped
API. Its test now checks PathLimit at Host registration, before fingerprints or
encoding can receive such a declaration. Malformed portable declaration/path lengths
and unresolved file references remain separate decoder cases. The wrong opaque-kind
case still rejects fingerprints and now checks the direct Host encoding boundary
instead of the removed path-authoring mapper. Validation was not bypassed or relaxed.

Removed the identity-representation generic from the Host standard-constraint
algorithm; its callers now pass fixed-ID Host values. The traversal, payload rules,
collection identity behavior and depth/node limits are unchanged. Its fixture uses
a registered Option owner in the same explicit context.

Validation:

- `cargo test -p kagari-types --lib host_interface:: --message-format=short --
  --nocapture`: all 17 selected Host cases pass, 27 unrelated cases filtered out.
  The previously carried 162 Types lib-test compilation errors are resolved, and
  the previously unrun scoped Host signature rejection case now executes and passes.
- After tightening the wrong-kind/package fixture isolation, the five
  `host_interface::type_declaration::tests` cases pass again. After fixing the Host
  constraint entrypoint to IDs, its one `host_interface::constraints::tests` case
  passes. Other unaffected cases retain the first run's evidence.
- `cargo check -p kagari-hir --lib --message-format=short` and the corresponding
  `kagari-contract --lib` check pass, covering the two affected semantic callers.
  `cargo clippy -p kagari-types --lib -- -D warnings` passes.
- Structure passes 1055 Rust files with zero violations or documented exceptions;
  formatting and diff checks pass. Reviewed test/production import scopes, bounded
  negative cases, table lifetimes and reference/semantic validation separation.

No workspace suite or backend matrix was run. HIR lib-test's previously recorded
141 migration errors, remaining Host consumer/example changes, common source fixture
lifecycle, BindingId remapping, SI04 integration and SI05/SI06 representation removal
and final documentation cleanup remain required. Types Host contract recovery is
focused boundary evidence, not complete plan acceptance.

### SI03 HIR fixture scopes and lib-test recovery

Recovered the HIR lib-test target after scoped declaration registration and Host
adoption. Host type/member/path fixtures now construct ID records in explicit
contexts and seal their metadata after registration. Offline query assertions
retain the analysis table; cross-context provider comparisons use ordinary checked
metadata import, including opaque type references and complete member/path records.
Same-database body reuse still compares IDs and arena facts directly. No whole-record
path projection or alternate semantic constructor was introduced.

Migrated callable, namespace, prelude and import Host inputs to the same lifecycle.
An immutable snapshot must be taken after constructors register every referenced
ID; several nested fixture constructors were hoisted during review to enforce this
order. Borrowed table views replace direct path-field inspection in member, payload,
generic, aggregate and Native binding expectations. Exact external identity queries
remain boundary lookups into the analyzed table rather than semantic path records.

The common source-checking fixture now receives a caller-owned DefinitionContext,
adopts its Host names into that context and retains it through subsequent queries.
It invokes the production implementation-member registration stage after signatures
and publishes the resulting prefix before aggregate checking. Language-role fixtures
adopt the complete Native provider batch before rendering/lowering, preserving the
installed provenance checks. Standard aliases and expected nominal/trait/callable
identities are looked up in the relevant retained table. Original failure, visibility,
revision, source location, generic ownership and checked-program assertions remain.

Focused local validation (all `cargo test -p kagari-hir --lib`, default profile,
features, target and parallelism):

- `host::`: 17 passed; `tests::typeck::`: 55 passed. The latter covers the changed
  shared checker and standard/trait fixtures, including const recovery, mutation,
  private method bounds, generic binder positions and collection key constraints.
- `language::items::tests`: 6 passed; `analysis::namespace_tests::`: 5 passed.
- One filtered invocation selects `analysis::callable_tests::`,
  `analysis::prelude_tests::explicit_host_declarations_take_precedence_over_the_helper_prelude`,
  `imports::tests::wildcard_import_expands_offline_host_module_declarations` and
  `imports::tests::source_host_and_module_item_ambiguities_are_rejected`: 12 passed.
- The previously blocked
  `analysis::prelude_tests::standard_variants_and_propagation_rebase_on_body_reuse`
  executes and passes. The plan's two initial identity filters,
  `published_cache_scopes_retain_prefixes_and_reject_foreign_queries` and
  `generic_parameter_identity_is_owner_and_position_based`, both pass.
- These are 98 selected tests, not the complete HIR suite. Intermediate compilation
  attempts reduced the carried 141 lib-test errors to 67, 42 and 21 before recovery;
  those attempts executed no tests. A final `--no-run --message-format=short` build
  passes after import cleanup, without warnings.
- Structure checks 1055 Rust files with zero violations or documented exceptions;
  formatting and diff checks pass. Reviewed changed test-only import scopes,
  registration/snapshot order and cross-scope reference validation. Production
  semantic implementations are unchanged at this checkpoint.

No full workspace suite or backend matrix was run. Remaining SI03 obligations
include the recorded aggregate/default/associated-family boundary selections and
identity visitor/remapping audit (notably BindingId body references). SI04 compiler
and MIR integration, downstream Host fixture/example callers, SI05/SI06 representation
removal and final documentation cleanup remain required. HIR lib-test recovery does
not establish compiler/runtime integration or overall plan acceptance.

### SI03 independent fresh analysis and deferred aggregate contracts

Completed the previously deferred default/override, trait identity, associated
constant, associated family and oversized-source selections after HIR lib-test
recovery. One trait contract invalidation fixture incorrectly passed a function ID
from the incremental database into an independent database. It now resolves the
same exact external identity in the fresh declaration snapshot before requesting
that body. Fresh/incremental fact comparison and retained old-return-type assertions
remain intact; foreign IDs are still rejected by the production query boundary.

Audited the remaining direct Native lowering-cache copy in the payload reuse
fixture. Copying adopted lowering into a new database failed with ForeignTable.
The fixture now independently adopts the provider batch and runs ordinary Native
import/correspondence validation. It retains identical immutable source/syntax
inputs (checked URI/text/revision and matched module identity) so navigation and
source-local parameter identities still compare exactly, while registration records
retain only the new context's IDs. Arena-qualified enum failure markers are mapped
by their checked syntax owner/slot to that retained input arena. It does not copy
old signature/body/query caches or share the definition writer.

The complete fresh aggregate catalog is imported with reference validation into
the comparison scope before equality. The existing full equality assertion,
including declaration locations and failure-variant semantics, is preserved.
An intermediate rebuild exposed distinct syntax arenas in Native parameter facts;
retaining the immutable syntax then exposed failure markers still targeting the
new arena. Both fixture ownership mismatches were repaired rather than dropping
fields from comparison or weakening equality to name/shape checks.

Focused validation, all with `cargo test -p kagari-hir --lib
--message-format=short`:

- Combined filters `analysis::trait_catalog_tests::`,
  `analysis::trait_identity_tests::`, `analysis::associated_const_tests::` and
  `oversized_source_module_identities_are_rejected_without_panicking`: 14 passed,
  one failed on the foreign fresh-database query. Rerunning only
  `imported_method_contract_edits_invalidate_consumers_and_preserve_old_snapshots`
  after repair passes, completing all 15 selected cases.
- Combined filters `analysis::gat_tests::` and
  `associated_definition_changes_invalidate_cached_body_and_keep_old_snapshot`:
  all 3 selected cases pass. The projection-bound tests already passed in the
  previous checkpoint's typeck selection and were not repeated.
- `body_edits_rebase_payload_references_and_match_fresh_facts`: the initial scoped
  input failure and subsequent complete-comparison failures were reproduced;
  the final run passes with full assertions. This completes 19 distinct selected
  cases at this checkpoint, in addition to the preceding checkpoint's 98.
- Structure checks 1055 Rust files with zero violations or documented exceptions;
  formatting and diff checks pass. Reviewed independent definition ownership,
  source versus semantic metadata, visitor import coverage for aggregate records,
  syntax arena references and unchanged production validation.

The recorded aggregate/associated/recovery selections are complete. The broader
SI03/SI06 identity visitor audit, including DeclarationId::Binding's body reference,
remains open. MIR inspection confirms its old mutable extraction still projects to
paths and verifies path-shaped records; SI04 must replace this with retained scoped
unverified metadata and migrate program linking/passes together. This inspection
made no MIR edits and did not repeat its unchanged known failing build. Downstream
compiler/runtime integration, representation-generic removal, selected source-free
and reload verification, documentation cleanup and exhaustive CI remain pending.

### SI04 scoped MIR verification, linking and editable extraction

MIR modules, functions, blocks, instructions and call contracts now default to IDs.
All production MIR verification and pass entrypoints consume fixed-ID records;
representation generics remain temporarily for the existing portable mapping layer
and must still be removed in SI06. Program diagnostics retain owned paths only as
explicit error output. No whole-record path projection remains in MIR production.

Single-module verification takes DefinitionMetadata<MirModule>: reference ownership
is established before semantic validation, and ABI names must already exist in that
prefix. Program verification takes fixed-ID modules and an explicit DefinitionContext.
It validates the complete batch against the original prefix before registering any
implicit ABI names, then checks module contracts, dependency closures, applications,
Host interfaces, Native declarations, selected calls and supplying modules. Names
alone confer no authority. Linked proof construction uses that same context and
successful modules/program retain its final immutable prefix. Verified fields stay
private; only verifier-owned validated state creates a seal.

Moved implicit ABI name collection to the existing contract owner and reused it from
MIR and bytecode. Both callers retain complete-batch reference validation before
name collection. The shared collector registers module roots, public functions/types,
traits and interface/inherent/private trait methods, preserving kind/name/occurrence,
allocation failures and cancellation. This replaces the prior MIR post-verification
path interning and avoids a second implementation of ABI name registration.

Editable module/program extraction retains its table and discards verification
facts. Owned module extraction transfers records directly; borrowed extraction uses
the existing cancellable record traversal with identity-preserving ID copies.
Optimization consumes the old seal, edits records in their retained scope and reruns
reference/semantic verification before using fresh liveness, roots, debug and flow
facts. It never rebuilds paths or uses ID equality as verification evidence.

The MIR codec now decodes file references directly to scoped raw records, explicitly
imports those IDs into a program construction context and runs ordinary program
verification. Canonical portable encoding, byte/count/type bounds, ABI/version
checks, no serialized seals and source-free dependencies remain unchanged. This is
a coupled SI05 boundary adjustment, not completion of private wire model removal.
The format/ABI identifiers were not bumped.

Validation and remaining integration:

- `cargo check -p kagari-mir --lib --message-format=short` passes after the coupled
  changes. Intermediate checks reported 131 and then 35 errors while fixed-ID
  applications, Native proofs, Host paths, ABI/layout queries and numeric contracts
  were being connected. Those MIR library errors are resolved.
- `cargo clippy -p kagari-mir -p kagari-bytecode --lib --message-format=short --
  -D warnings` passes, including the shared registration dependency. An earlier MIR
  run identified 17 obsolete Copy-ID clones and two redundant borrows; all were
  removed without lint allowances. `cargo build -p kagari-mir --lib` also passes.
- `cargo test -p kagari-mir --doc --message-format=short`: both compile-fail seal
  examples pass. Their paths now name the real public modules, so rejection tests
  raw-to-verified conversion and mutation through the immutable seal rather than
  nonexistent root-level names.
- A temporary source-free public-API probe, compiled with rustc against the default
  workspace build artifacts, passes. It constructs one private i32 function with
  two constant loads and a return, and a valid root lexical debug scope. It verifies
  the graph, roundtrips canonical MIR bytes into an independent table, checks an old
  prefix rejects a later definition, and optimizes away exactly one unused load
  while retaining the function ID and table identity. Both owned/borrowed editable
  extraction retain the scope; deleting the terminator is rejected by revalidation.
  Supplying original IDs to an independent context is rejected; explicit metadata
  import followed by verification preserves canonical bytes. Cancelled copy, decode
  and optimization operations reject. The probe was repeated after consolidating
  registration and preserving cancellable copying. It is local diagnostic evidence,
  not a new retained smoke test or a replacement for the existing codec/pass owners.
- `cargo test -p kagari-compiler --lib
  portable_program_codec_rebuilds_seals_and_preserves_canonical_lowering
  --message-format=short` cannot compile the lib-test target: 873 migration errors;
  the selected test did not execute. The errors include 417 source/lower diagnostics,
  30 source/program diagnostics, removed MIR paths/scope consumers in bytecode
  defaults/interfaces, the new verification context and metadata extraction APIs,
  native-input checking and old path-shaped test fixtures. SI04 owns source-free
  bytecode/default/interface assembly first, then specialization/source lowering and
  affected fixtures; SI05/SI06 own remaining codecs/representation removal. Do not
  restore path projections or weaken the failing contracts.

Structure passes 1056 Rust files with zero violations or documented exceptions;
formatting and diff checks pass. Manual review covers imports, private seal creation,
reference visitors, table-prefix validation before registration, cancellation,
source-free production dependencies and diagnostic-only path exports. The structure
parser initially treated `&raw` as a raw-reference prefix; renaming that argument to
`inputs` resolved its parse finding without an exception or checker change. No full
workspace/backend suite or performance measurement was run. Compiler integration,
selected end-to-end/source-free/reload tests, the HIR BindingId visitor audit and
remaining SI06 generic removal/documentation cleanup prevent overall acceptance.

### SI04 scoped bytecode assembly and immutable linked proofs

Frontend-free compiler emission now returns DefinitionMetadata<BytecodeModule> or
DefinitionMetadata<BytecodeProgram> in the original verified MIR scope. Default
method selection borrows the closure's ABI, Host and enum records directly;
interface arguments, binder scope, assumptions and selected Native instances remain
IDs throughout. Interface parent/view population receives the same table. Native
input comparison validates the retained records and encodes their ordinary portable
projection, preserving exact canonical byte comparison and floating-point payloads.
No compiler bytecode/default/interface/native-input whole-record path conversion
remains. Editable emission does not carry an execution seal.

Inspection exposed an ownership defect in the previous coupled migration:
ProofCatalog allocated implicit names while validating contracts. A compiler holding
an immutable MIR snapshot could therefore not use that checker without recovering a
writer or rebuilding a path-shaped catalog. ProofCatalog now accepts a retained
DefinitionTable and performs read-only queries. The existing contract registration
owner collects language protocol/associated-output names and Host implementation
occurrences; ABI methods already belong to that collector. Native catalog creation
and import collect their implicit names before proof construction. Method and Host
implementation lookup rejects missing names; registration alone still supplies no
proof of declaration validity or implementation applicability.

Bytecode module/program verification likewise reads a DefinitionTable. Artifact and
VerifiedBytecodeProgram construction own name collection before calling verification.
Both collection and read-only verification validate all input references against the
original prefix before semantic checks; collection checks the complete module batch
before appending names. Foreign IDs and IDs beyond the retained prefix cannot become
valid merely because collection later appends entries. MIR's linked checker now
borrows the same immutable prefix used for module verification, so final sealing no
longer refreshes each module's metadata after proof construction. Cancellation is
still checked before publishing the seal. No seal fields or unchecked construction
entrypoints were exposed, and no ABI/format identifiers changed.

Focused validation:

- `cargo check -p kagari-compiler --no-default-features --lib --message-format=short`
  and `cargo build -p kagari-compiler --no-default-features --lib --message-format=short`
  pass. This selects executable lowering/native-input processing without the source
  compiler. `cargo clippy -p kagari-compiler --no-default-features --lib
  --message-format=short -- -D warnings` also passes, including the affected MIR,
  bytecode and contract dependencies. The final run follows the removal of MIR's
  redundant post-proof metadata refresh.
- `cargo check -p kagari-runtime --lib --message-format=short` and
  `cargo clippy -p kagari-runtime --lib --message-format=short -- -D warnings` pass
  for the Native catalog registration/proof caller change.
- A temporary source-free public-API probe, compiled with rustc against the default
  workspace build artifacts, verifies single-module/program lowering retains MIR's
  exact table and function ID. Independent MIR decoding produces identical canonical
  bytecode bytes. A later ID substituted into the lowered function and a foreign
  table are rejected. Explicit adoption into a new context followed by ordinary
  bytecode sealing allows Native input verification; changing constant 42 to 43 in
  otherwise valid sealed bytecode produces NativeInputError::Mismatch. Cancelled
  Native verification rejects. The preceding MIR snapshot, optimization, codec,
  seal invalidation and cancellation assertions also pass in that probe.
- A temporary scoped adaptation of the existing native_defaults fixture verifies
  generic Native default selection retains the registered template ID, concrete i32
  argument/signature and Native binding. Catalog construction leaves the writer's
  length unchanged. The prefix containing all explicit fixture references but no
  implicit methods, an independent table, and a catalog lacking the required Native
  template each reject. These probes are diagnostic evidence, not retained duplicate
  smoke tests or substitutes for the existing broader codec/default-method owners.
- `cargo check -p kagari-compiler --lib --message-format=short` still fails with 430
  source compilation errors. Representative failures are source/program treating
  DefinitionMetadata<AnalyzedModule> as a bare record and passing the old MIR
  verification arguments; source/lower ABI and specialization helpers still expect
  path-based types/identities. No source compiler tests executed. SI04 owns migration
  of that closure/specialization context and its consumers, followed by the already
  recorded selected compiler tests. The prior 873 lib-test failure and these 430
  library diagnostics are different build scopes, not comparable error counts.

Structure checks 1056 Rust files with zero violations or documented exceptions;
formatting and diff checks pass. Manual review covers read-only verifier imports,
borrowed default contracts, exact table-prefix ownership, implicit registration,
private seal creation and no production re-exports or path projections. No workspace
suite, complete backend matrix or performance measurement ran. Source specialization,
remaining registration/test consumers, the HIR BindingId visitor audit, codec/private
wire and representation-generic removal, selected source-free/reload integration and
final documentation cleanup remain pending under SI02-SI06.

### SI04 source specialization and canonical scoped contracts

Source lowering now carries DefinitionId through instance keys, generic arguments,
ABI contracts, Native imports, callable/result adapters, Host bridges, layouts and
cross-module specialization demands. Module ownership, containment, associated
members and protocol/default selection query the retained table. Diagnostic exports
alone reconstruct owned paths. The existing semantic and implementation-search
budgets remain in force; no format or runtime ABI identifier changed.

CheckedAnalysis exposes its shared append-only DefinitionContext for compiler-owned
generated identities. Lowering retains the checked IDs and allocates closure,
resume, callable, derived-protocol and Host-bridge occurrences through that writer.
For example, compiling a captured closure preserves its enclosing function ID and
adds the closure function ID; the original checked snapshot cannot resolve that new
ID. The generated MIR receives a new prefix and ordinary verification before it
can carry evidence. A cloned writer cannot allocate conflicting meanings, and
writer access neither expands earlier snapshots nor grants a verification seal.
This avoids reconstructing a second allocation scope from owned paths. It does not
complete the outstanding generic HIR mapper/BindingId audit: whole-HIR adoption into
another context must still repair those visitors and their retained internal tables.

Two integration failures exposed assumptions hidden by path ordering:

- Source ABI bounds and substituted Native requirements were ordered by numeric
  DefinitionId while validators require canonical semantic order. ABI emission and
  Native substitution/comparison now use the existing scoped GenericBound/Constraint
  normalization owner, preserving exact requirement equality. The obsolete generic
  substitution sorting remains an SI06 removal target; there is no second semantic
  normalization implementation.
- MIR and bytecode interface instantiation still used the legacy no-table entrypoint.
  Valid generic collection interfaces therefore failed their retained-scope checks.
  The affected verification, access and trait-bound callers now pass their actual
  DefinitionTable to scoped instantiation. Scope/binder checks were not bypassed.

Derived enum Hash tags encode the reachable canonical identity table, nominal type
and variant file reference. They never serialize process-local IDs. This explicit
portable boundary preserves nominal/variant identity across independent allocations
and variant slot reordering, including enums containing custom Hash payloads.

Focused validation:

- `cargo check -p kagari-compiler --lib --message-format=short` and
  `cargo clippy -p kagari-compiler --lib --message-format=short -- -D warnings` pass.
  This resolves the previously carried 430 production source compiler diagnostics.
- `cargo test -p kagari-compiler --test language_foundation --message-format=short
  --no-fail-fast` passes all four existing cases. They cover scalar lowering, Native
  defaults, collection interface iteration and foundational generic/associated
  contracts. Initial failures identified the canonical-bound and missing-table
  defects above; the same assertions pass after those repairs.
- A temporary public-API adaptation of existing source/codec scenarios checks a
  captured closure's retained scope and frozen input prefix, drops source/analysis
  owners before independent MIR decoding, compares canonical re-encoded MIR and
  portable bytecode, and verifies bytecode. A custom-Hash enum produces identical
  identity tags in independent analyses and after reversing variant slots. All
  assertions pass. These diagnostics do not replace the retained semantic owners.
- The preceding source-free MIR/bytecode probe passes again with the affected
  libraries: retained IDs, old-prefix/foreign-table rejection, independent codec
  correspondence, explicit adoption, changed Native-input mismatch and cancellation.
- `cargo test -p kagari-compiler --lib
  tests::codec::portable_program_codec_rebuilds_seals_and_preserves_canonical_lowering
  --message-format=short --no-fail-fast` remains blocked at test-target compilation
  by 439 errors. Representative remaining fixtures access metadata as bare records,
  mutate old owned identity paths, construct default path-shaped types and use old
  verification signatures. No selected lib test executed. SI04 owns the affected
  compiler fixture migration; this is distinct from the passing production library
  and integration target. Do not rerun the unchanged failure before those repairs.

Structural review covers grouped explicit imports, generated-name ownership,
scope-aware normalization/instantiation, retained prefixes, diagnostic-only paths,
portable Hash encoding and unchanged search budgets. No re-export, compatibility
entrypoint, lint allowance or structural exception was added. Structure, formatting
and diff checks pass. No full workspace/backend suite or performance measurement
ran. Remaining registration/backend/test adoption, HIR visitors, private wire and
representation-generic removal, selected reload/source-free integration and final
documentation cleanup remain open; SI02-SI06 acceptance is not claimed.

### SI03 fixed HIR models and SI06 representation removal

HIR no longer declares or consumes identity-representation parameters. Types,
substitutions, declaration IDs, signatures, aggregate/import catalogs, selected
calls, type tables and analysis bundles use DefinitionId directly. Shared semantic
declarations imported from kagari-types still use that owner's transitional
DefinitionId instantiation; removing the shared owner's generic remains SI06 work.
No HIR path/wire model or compatibility alias replaces the removed representation.

The visitor audit exposed two different capabilities previously coupled by
DefinitionRecord/Rebind. Common's DefinitionReferences validates retained ID fields
and keys; RemapDefinitions rebuilds independently reusable fixed-ID records. Metadata
construction needs only the visitor, while import_into needs the remapper and returns
the same record type. Transitional blanket implementations delegate to existing
generic owners without a second semantic engine. Remove those adapters when the
remaining types/contract/executable representation owners migrate in SI06.

Binding declaration traversal now includes BindingId.body. Remapping changes that
scoped function/constant reference while preserving AnalysisId and local slot; the
ordinary key rebuilding still rejects collisions. A foreign table cannot validate a
binding merely because the former visitor ignored its body. Type shape limits and
cancellation remain enforced by both traversal and remapping.

Whole analyzed/prepared/declaration bundles retain source arenas, namespace inputs,
registration revisions and declaration indexes. The previous arbitrary mapper cloned
their context/Host/import owners while changing selected ID fields. That incomplete
operation is removed, not repaired by constructing another portable HIR hierarchy.
These containers expose complete reference validation and same-scope editable
extraction only. Independently reusable signatures, type tables, aggregates and
individual declaration records retain ID-to-ID remapping. Visitors now also cover
attached Native lowering facts, resolved import bridge IDs, Host contracts/signatures
and Host index/origin keys. Source/Host slot handles are unchanged. The lasting
contract is documented in the HIR architecture guide.

Selected-body queries now accept scoped IDs. The function cache shares the analysis
definition writer and uses ID insertion, lookup and retention; it no longer exports
paths into a separate cache identity context. External path queries explicitly look
up the identity in the destination declaration snapshot. This preserves independent
fresh-analysis comparisons and foreign-ID rejection, with revision/dependency checks
still governing cache reuse. Namespace/navigation queries likewise take fixed
DeclarationId values and validate their retained prefixes.

Focused local validation:

- Compiler library check and `cargo clippy -p kagari-compiler --lib
  --message-format=short -- -D warnings` pass, including the changed common/HIR
  dependencies. No production HIR DefinitionReference or Rebind remains.
- `cargo test -p kagari-hir --lib analysis::body_queries::tests
  --message-format=short --no-fail-fast` passes nine existing cases: selective body
  checking, incomplete-member recovery, constants, imported/associated invalidation,
  same-name implementations, stale/deleted queries and retained snapshots.
- The existing binding-identity and published-cache-prefix cases pass with their
  new fixed-ID assertions. Binding metadata rejects a foreign table; importing and
  restoring it changes/restores body IDs while retaining analysis/local identity.
  These ran under analysis::identity_tests before the structural split; their current
  owner is analysis::scoped_identity_tests. The latter module's
  independently_constructed_hir_types_observe_conversion_bounds case also passes,
  preserving depth rejection and cancellation without an obsolete HIR path conversion.
- `cargo test -p kagari-compiler --test language_foundation --message-format=short
  --no-fail-fast` passes all four existing cases after the HIR representation change.
  This checks the directly affected source-to-MIR/bytecode dependent boundary.
- The first HIR lib-test build reported 28 fixture errors from old mapper bounds and
  path-taking body queries. All were repaired at their existing owners; fresh analyses
  resolve paths in their own snapshots rather than reusing foreign IDs. The selected
  tests above executed after repair. The preceding compiler-lib fixture failure is
  unchanged and was not repeated.

Manual structural review covers fixed reference fields, remapped unique keys,
unchanged source/body handles, namespace/registration ownership, explicit imports
and cancellation/limits. Splitting scope/metadata cases from navigation identity
tests resolves the 1200-effective-LOC limit without exemptions. Structure checks
1060 Rust files with zero violations or exceptions; formatting and diff checks pass.
No workspace/backend matrix or performance measurement ran. Shared semantic and
executable representation removal, remaining consumers/codecs, selected source-free
and reload integration and final plan cleanup remain open.


### SI06 fixed shared semantic algorithms

TypeSubstitution, ImplementationPattern, projection normalization/lookup, trait
ancestry and callable/default requirement application now use DefinitionId directly.
Aggregate layout application, MIR structure selection and bytecode Native result
target selection follow the same fixed-ID semantic boundary. No alternate path
algorithm replaces them. Shared serializable model parameters remain until their
owner-local wire records migrate together; this checkpoint does not claim their
removal.

Scope checking now has one types_in_scope entrypoint with a mandatory retained
table. InterfaceTable::instantiate likewise takes both the caller binder scope and
table; its path-only and optional-table wrappers are removed. Range-bound checking
uses the same explicit scope. Direct table parent/kind queries preserve nominal,
associated-member and foreign-prefix rejection without reconstructing paths.

Bound substitution now delegates to GenericBound::normalize, the existing owner of
canonical semantic ordering. Previously its local BTreeMap sort used numeric ID
order, requiring callers to normalize again. The source Native import and linked
Native checker repairs, plus the Native default builder's duplicate normalization,
are removed. All proof, MIR/bytecode verification, registration and stdlib consumers
supply their actual retained tables. Transform limits, once-only substitution,
family binders and cancellation are unchanged; fixed IDs no longer pass through a
representation-dependent path-limit predicate. Identity construction/retained-table
validation still owns identity limits and scope admission.

Focused local validation:

- `cargo check -p kagari-compiler -p kagari-runtime --lib --message-format=short`
  and the corresponding `cargo clippy ... -- -D warnings` pass. These cover the
  changed shared algorithms and directly affected production consumers.
- `cargo test -p kagari-types --lib ty::substitution::tests
  --message-format=short --no-fail-fast` passes seven existing tests for binder
  substitution, associated outputs/cycles, expansion/depth limits and cancellation.
  The existing canonical bound merge case now allocates constraint IDs in reverse
  spelling order, proving the output cannot depend on allocation order.
- `cargo test -p kagari-types --lib declaration::verify::scoped_tests
  --message-format=short --no-fail-fast` passes four existing cases for nominal/Host
  kinds, associated-member ownership, retained/foreign scopes and canonical bounds.
  Removed path/no-table assertions belonged solely to the deleted entrypoints;
  semantic rejection assertions remain at the fixed-ID owner.
- `cargo test -p kagari-compiler --test language_foundation --message-format=short
  --no-fail-fast` passes four cases, including Native defaults, collection interface
  iteration and generic associated/static calls through source, MIR and bytecode.
- Before this checkpoint's edits, `cargo check -p kagari-codegen-cranelift --lib
  --message-format=short` passed against the preceding committed source/HIR changes.
  This resolves the uncertainty about that production backend library, not backend
  execution or its old fixture consumers; no complete backend matrix ran.

The first selected types test build reported 23 old path-fixture/entrypoint errors;
those fixtures now construct explicit scoped IDs and all selected tests executed.
Intermediate production caller/type errors and copy-ID lint findings are repaired.
The previously recorded compiler lib-test fixture failure remains open and was not
repeated. Shared/executable model and codec migration, remaining fixture/backend/SDK
adoption, source-free and reload integration and final documentation cleanup remain
SI02-SI06 obligations.

Manual structural review covers the changed production imports, ownership, direct
parent queries, deleted wrappers, shared canonicalization and unchanged traversal
budgets. No production glob, new re-export, compatibility API or structural exception
was added. Structure checks 1060 Rust files with zero violations or exceptions;
formatting and diff checks pass. No full workspace suite or performance measurement
ran; exhaustive feature/backend acceptance remains CI scope.

### SI05/SI06 runtime entrypoints and fixed-ID adoption

Runtime and VM interface invocation now take DefinitionId directly. Lookup validates
that ID against the receiver snapshot's supplying module before selecting a linked
method or parent view. Native CallContext/StorageContext type resolution and
Runtime::resolve_type_arguments accept only scoped types. They visit references
against the loaded generation's immutable definition prefix before normal type
argument preparation; replacing the old identity mapper does not admit foreign or
later runtime IDs. Managed payload field preparation likewise validates borrowed
fixed-ID types instead of allocating an identity-preserving mapped copy. Vec's
symbolic authoring identity resolves through explicit table lookup.

Native bindings/catalog validation use DefinitionReferences. Native typed-reference,
builder, catalog and verified-program adoption use RemapDefinitions; common's
DefinitionMap::import_records now needs that fixed-ID capability rather than a
DefinitionRecord/Rebind equality constraint. Imports still include keys, referenced
values and ancestors, reject invalid scopes, and preserve the original verified
version/dependency ownership. Runtime and VM source contain no DefinitionReference,
DefinitionRecord, Rebind, map_identities or whole-record to_paths dependency. The
common blanket adapters remain solely for shared/executable models pending SI06.

The VM production check exposed two carried reload wrappers still accepting bare
program/artifact records. They now retain DefinitionMetadata through staging and
ordinary runtime verification/publication. Existing native_execution fixtures now
construct explicit scoped records, including malformed artifact inputs whose identity
scope is valid while their content hash is deliberately invalid. The existing VM
interface fixture resolves methods by child lookup in its loaded generation and
reads lowered records through their metadata owner. Assertions and execution/GC
contracts are retained.

Focused local validation:

- `cargo check -p kagari-vm --lib --message-format=short` and
  `cargo clippy -p kagari-vm --lib --message-format=short -- -D warnings` pass,
  including the changed common/runtime dependencies.
- `cargo test -p kagari-runtime --test native_execution scoped_programs
  --message-format=short --no-fail-fast` passes the existing independent-context
  identity import, foreign type rejection, version/epoch distinction, reload and
  retained type-argument ownership case. It passes again after adoption moves to
  the fixed-ID remapper.
- The same target with filter `reload` passes two existing cases: a malformed
  reload preserves installed code/version, and a Native handle pins the entire
  dependency program across reload. Filter `shared_verified_program_identity`
  passes the independent-runtime shared-version case.
- `cargo test -p kagari-runtime --test native_registration scoped_type_inputs
  --message-format=short --no-fail-fast` passes the existing retained-prefix,
  foreign-ID and cross-context registration case after map/catalog adoption changes.
  This normal Cargo route now builds; the earlier HIR/MIR blockers described in
  the registration checkpoint no longer apply to this selected target.
- `cargo test -p kagari-vm --test interface_allocations
  cached_native_receiver_defaults_release_their_program_and_gc_values
  --message-format=short --no-fail-fast` passes the existing selected Native default
  invocation, rooting, collection and generation-retention cleanup case. Allocation
  measurement cases in that target were filtered out; no measurement claim is made.

The initial native_execution target build reported 18 obsolete bare-record/path and
verification-signature errors. These are repaired; all six selected runtime/VM cases
above executed. The VM interface target's old to_declaration().unwrap() fixture call
is also repaired. Previously passing cases were not rerun solely to refresh a log.
The preceding compiler lib-test fixture failure and remaining consumer/codecs/tests
are still open; this is focused integration evidence, not SI02-SI06 completion.

Manual review covers fixed entrypoint ownership, retained-prefix checks, complete
catalog key/value imports, roots, original verification versions and import grouping.
No compatibility wrapper, new semantic representation, re-export or structural
exception was added. Structure checks 1060 Rust files with zero violations or
exceptions; formatting and diff checks pass. No full workspace/backend matrix ran.


### SI05 private flat type-set wire records

The existing PortableTypes boundary now retains codec-private WireType records,
not Ty<PortableDefinitionRef>. Encoding walks scoped types directly into bounded
flat nodes using DefinitionEncoding. Decoding reads bounded node vectors and uses
DefinitionDecoding while constructing scoped types; no file-reference semantic tree
or intermediate path model is constructed. This is the first shared wire leaf
boundary; Host value types and declaration/contract/MIR/KBC aggregate wire owners
still need their coupled migration before semantic representation parameters can be
removed. PortableTypes is a standalone type-set API, not a claim that those aggregate
artifact paths have all migrated.

The former flat type writer/builder is shared by this explicit conversion and the
remaining generic Serde consumers, avoiding another tree codec. Private Node has no
default path representation. Node/child limits, depth limits, missing/trailing node
rejection and associated-binding uniqueness/order remain enforced. The private wire
Serde reader validates flat counts; explicit decode validates shape and references
before returning scoped records. Canonical binding validation checks file references
before mapping, and rebuilding the ID-keyed map rejects collisions. Cancellation is
checked in the explicit encoder/decoder; ordinary limit checks and legacy Serde do
not allocate a new cancellation token merely to share the traversal. No process-ID
Serde, global context, public wire re-export or version bump was introduced.

Associated binding keys and value subtrees must move together when ID allocation
order differs from file order. Encoding now orders the pairs by mapped references,
then emits the corresponding children. The existing canonical-byte test allocates
all referenced identities in reverse order, including members within the same trait;
its bytes still match an independent context with normal allocation and ignore
unrelated definitions. Existing malformed-wire cases now exercise the private wire
path, including duplicate/reversed binding order, excessive node/binding counts,
missing children, trailing nodes and excessive depth. Identity tests retain foreign
scope and forged file-reference rejection and exercise cancelled explicit decoding.

Focused local validation:

- `cargo test -p kagari-types --lib ty::identity::tests --message-format=short
  --no-fail-fast` passes five existing cases for complete references, canonical bytes,
  foreign/forged identities, explicit adoption, binder ownership and cancellation.
- `cargo test -p kagari-types --lib ty::wire::tests --message-format=short
  --no-fail-fast` passes three existing composite/associated/invalid-node cases.
  The final run also checks the removal of Node's obsolete default path parameter.
- `cargo clippy -p kagari-types --lib --message-format=short -- -D warnings` and
  `cargo build -p kagari-compiler --lib --message-format=short` pass. The compiler
  build selects a directly affected consumer of the shared type Serde traversal.
- The previously documented source-free MIR/bytecode public-API probe passes against
  the rebuilt library artifacts: scope retention, canonical independent codec
  correspondence, explicit adoption, foreign/later-reference rejection, Native input
  mismatch and cancellation. This checks shared-parser integration while the existing
  compiler lib codec fixture target still awaits its recorded migration; the probe
  does not replace that retained acceptance owner.

Manual review covers paired key/value order, source-reference validation before
remapping, bounded flat reads and bounded recursive reconstruction, error propagation,
private visibility and explicit imports. A structure finding on A::Error::custom was
resolved by the existing imported Serde error trait. Structure, formatting and diff
checks pass with 1060 Rust files and no violations/exceptions. No full workspace or
backend matrix, generated-artifact refresh or performance measurement ran. Shared
semantic model parameters and remaining generic Serde support must be removed with
the aggregate private wire owners in SI05/SI06; final integration/docs cleanup and the
previous compiler fixture failure remain open.


### SI05 private Host envelope/value records and offline integration

KHI encoding/decoding and all Host fingerprints now use private wire records for
serialized declarations, members, trait bindings and path declarations. The encoder
visits scoped references, constructs the canonical table and explicitly writes local
references. The decoder reads bounded records, validates the file table, resolves
references directly to IDs, and applies the existing Host contract checks before
returning scoped metadata. It does not build HostInterface/HostValueType with file
references, use arbitrary Rebind for codec conversion, or create execution evidence.
The private Encode trait only selects an owner's fixed serialized record.

The flat Host value reader shares its bounded node reader and shape builder with
remaining generic Serde consumers. Nodes no longer have a default path parameter.
Envelope field order, value tags, fingerprint domains, declaration/member/node/depth
and byte limits are retained; no format identifier changed. Per-record private wire
conversions preserve ordered fields, parameters, trait arguments and path segments.
Generic Host semantic models/Serde and other aggregate codecs remain SI05/SI06 work;
this checkpoint does not claim their migration is complete.

Existing malformed-node and forged-reference cases now target private wire records;
collection round trips use the real KHI envelope. The existing canonical test imports
into reversed type allocation order with an unrelated root and compares bytes and
fingerprints. The offline_host integration target now constructs IDs in explicit
contexts, retains metadata scopes and uses explicit imports. Its obsolete path
constructors, generic rebinding helper and path mutation were removed; all assertions
for callback suppression, handle ownership, old snapshots and rejected publication
remain. Its stale permissions/cost test name now describes identity/signature/borrow/
effect checking.

Focused local validation selects the changed Host codec contracts and their runtime
registration/link/reload consumer, not a workspace sweep:

- `cargo test -p kagari-types --lib host_interface:: --message-format=short
  --no-fail-fast`: 17 cases pass; the canonical case also passes after strengthening
  allocation-order coverage.
- `cargo test -p kagari-runtime --test offline_host --message-format=short
  --no-fail-fast`: all eight cases pass. Initial fixture compilation found five
  residual Option declaration-constructor errors; explicit context interning resolves
  them and the rerun executes every selected case.
- Private wire associated types initially had narrower visibility than the private
  Encode trait (E0446); they now share the Host module boundary. The structure checker
  incorrectly treated `pub(in crate::host_interface)` as an importable use-site path.
  It now excludes only visibility syntax, retaining field/signature/import/re-export
  checks; its 43 self-tests pass. No visibility expansion outside Host or structural
  exception was used.
- `cargo clippy -p kagari-types --lib --message-format=short -- -D warnings` and
  `cargo clippy -p kagari-runtime --test offline_host --message-format=short --
  -D warnings` pass. The selected integration build also compiles its compiler and
  standard-library dependencies; this is not full compiler/backend acceptance.

Manual review covers every serialized declaration field and leaf tag, reference
collection, decode ownership, limits, retained validation, private visibility,
production imports and unchanged fingerprint domains. Structure checks 1063 Rust
files with zero violations/exceptions; formatting and diff checks pass. No full
workspace/backend matrix, artifact regeneration or performance measurement ran.
The prior compiler lib-test fixture failure remains open, along with aggregate wire
migration, shared model parameter removal and final integration/document cleanup.

### SI05 shared Host wire owner for required ABI fingerprints

KBC's required-Host-ABI fingerprint was still independently rebinding HostInterface
to file-reference semantic records. It now collects the scoped reference closure
and uses HostInterfaceRecord from the existing Host wire owner. That opaque record
has private fields and a fixed-ID encode API taking an explicit DefinitionEncoding;
its nested records and decoding remain internal to the Host owner. This is an actual
cross-crate codec consumer, not an unused forwarding layer or a second semantic
implementation. The module is documented as codec-only. No new re-export is added.

KBC retains its existing policy: validate the complete supplying scope, omit paths,
clear declaration/member documentation, sort the declaration set by exact identity,
and hash the same domain plus ordered record/table fields. The codec owns only
bounded value shape and reference conversion; it does not grant Host validity or
execution evidence. KBC/MIR aggregate envelope conversion and shared semantic model
generics remain outstanding. The broader of_record rebinding API still serves those
unmigrated executable/public-item records and remains an SI05/SI06 removal target.

The existing required-ABI test also checks type/member documentation exclusion,
member type sensitivity and foreign snapshot rejection. Its independent context
already preallocates references in a different order. A stale constructor in the
same identity target caused five compile errors before test execution; the virtual
path fixture now creates its Host root in the explicit context and no longer uses
scope_record. Its canonical bytes, scope retention, cross-context import, verification
and forged-reference assertions are preserved.

Focused local validation:

- `cargo test -p kagari-bytecode --test identity --message-format=short
  --no-fail-fast`: six cases pass after that fixture repair, covering canonical ABI
  hashing, artifact verification, resource limits, malformed envelopes and retained
  path identity. The initially filtered fingerprint command did not execute tests
  until the target's constructor error was repaired.
- `cargo test -p kagari-types --lib host_interface::wire:: --message-format=short
  --no-fail-fast`: the existing forged KHI reference case passes after the record
  rename.
- `cargo test -p kagari-runtime --test offline_host
  module_load_and_reload_require_matching_bindings_before_publication
  --message-format=short --no-fail-fast`: one directly affected publication/reload
  case passes; unrelated cases are filtered.
- `cargo clippy -p kagari-bytecode --lib --message-format=short -- -D warnings`
  passes, including the final restriction of the unneeded cross-crate decode API.

Manual review verifies private field ownership, explicit imports, the retained
source-scope check before fingerprint exclusions, scalar/field serialization order,
error propagation and the unchanged hash domain. Structure checks 1063 Rust files
with no violations/exceptions; formatting and diff checks pass. No workspace/backend
matrix or performance measurement ran. The previously recorded compiler lib-test
fixture failure and the remaining coupled aggregate/model migration remain open.

### SI05 shared public ABI declaration records and fingerprint consumers

Public ABI hashing no longer instantiates PublicItem or its declarations with file
references. Opaque PublicItemRecord at kagari-contract composes fixed records for
functions, constants, data types, traits and interface/inherent tables. kagari-types
owns their signature, binder, constraint, default-application, conversion-adapter,
member and associated-output wire records. Type expressions reuse the existing flat
WireType encoder/builder. The codec-only modules expose the concrete records actually
needed across crates; internal leaves and fields stay private, with no re-exports.

Reference conversion uses explicit DefinitionEncoding/DefinitionDecoding and checks
cancellation. Bounded sequence/map readers retain their existing limits. Nominal
associated-output entries move keys and value records together into file-reference
order and reject collapsed keys when rebuilding. Nested type nodes retain their
existing shape/depth/canonical binding checks. Record/enum field and tag ordering is
preserved; format identifiers are unchanged. Decode returns scoped unverified facts,
not declaration validation or execution evidence. There is no new semantic algorithm
or public API for arbitrary identity-representation conversion.

Artifact metadata and runtime reload now call of_public_item. The public generic
ArtifactFingerprint::of_record API is removed; program/module/artifact consumers
use fixed-input entrypoints. Their existing rebinding implementation remains private
until the executable aggregate wire owners migrate. NativeDeclaration and complete
ModuleContract/MIR/KBC aggregate codecs still require their coupled conversion;
shared semantic representation parameters and old generic Serde remain SI05/SI06
work. These declaration records do not claim full artifact decoding migration.

The new public_abi_wire integration target owns this shared codec boundary. It
round-trips each public-item category, including struct/enum data, nominal output
bindings, default applications and associated members, through independent file
scopes and explicit adoption. Reverse allocation of declarations and associated keys
with different value types must produce identical bytes. It also checks foreign
referenced inputs, cancellation and rejection of an excessive generic count before
reading elements. The first run incorrectly expected a reference-free constant to
reject an empty foreign encoding; the scope assertion now targets a function with
actual references. Production validation was not relaxed.

Focused local evidence:

- `cargo test -p kagari-contract --test public_abi_wire --message-format=short
  --no-fail-fast`: both codec boundary cases pass after the fixture correction.
- `cargo test -p kagari-bytecode --test identity
  required_host_fingerprint_is_derived_and_independent_of_docs_and_order
  --message-format=short --no-fail-fast`: the existing artifact/Host case passes,
  including its updated fixed artifact-fingerprint entrypoint.
- `cargo clippy -p kagari-types -p kagari-contract -p kagari-bytecode
  -p kagari-runtime --lib --message-format=short -- -D warnings` passes for the
  four changed production owners. `cargo clippy -p kagari-contract --test
  public_abi_wire --message-format=short -- -D warnings` also passes.
- `cargo test -p kagari-runtime --lib
  reload_rejects_public_abi_changes_before_publication --message-format=short
  --no-fail-fast` does not execute tests: the retained library-test target reports
  178 old fixture compile errors. Representative causes are removed
  HostPathSegmentRegistration imports, old Host constructors/Native builders and
  bare BytecodeProgram/KbcArtifact inputs where scoped metadata is now required.
  SI06 owns these fixture repairs; this is a carried failure, not a passing reload
  test. Do not repeat that unchanged target until its fixtures are repaired.
- `cargo build -p kagari-runtime --lib --message-format=short` passes. A temporary
  public-API probe reproduces the selected reload scenario against those libraries:
  create one module with a public Script function `main`, no parameters/generics,
  and builtin I32 result; wrap the program in checked metadata from an explicit
  context and load it. Change only the public result to String in a second scoped
  input. of_public_item changes, staging/publishing rejects it with
  KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH, and loaded count/latest epoch remain
  unchanged. This is behavioral evidence, not a replacement for repairing the
  retained library-test owner. No source compiler is used by this probe.

Manual review covers every copied field/tag, ordered vectors, nominal key/value
pairing, cancellation/error propagation, codec-only visibility, imports and separate
verification ownership. Structure checks 1069 Rust files with zero violations or
exceptions; formatting and diff checks pass. No full workspace/backend matrix,
artifact regeneration or performance measurement ran. The prior compiler lib-test
fixture failure, newly observed runtime lib-test fixture failure and remaining
aggregate/model migration are all open; final SI integration is not complete.


### SI05 complete MIR wire aggregate

MIR encoding/decoding now uses codec-owned fixed ProgramRecord, module, function,
block, instruction and call/path/field records. The aggregate composes opaque
records at the existing owners: Native declarations/signatures at types; layouts,
module ABI, semantic slots and generic/interface/Native callable facts at contract;
Host declarations/paths at types. Nested implementation-only records remain private
or crate-visible. No forwarding facade, re-export, generated include or second
semantic validator was added.

The codec no longer clones a complete semantic program just to rebind it to file
references. RawProgram has fixed IDs and only explicit reference visiting and
ID-to-ID remapping for adoption. ProgramRecord collects the referenced closure from
the verified owner's retained snapshot, encodes through DefinitionEncoding and
writes the same root/modules/table field order. Decoding validates the table,
resolves records through DefinitionDecoding and retains it with checked metadata;
the public decoder still imports into a fresh verification context and rebuilds
all analyses. Bounded readback before encoding publication, count/byte limits,
header/ABI checks, trailing-byte rejection and executable verification remain.

Native/type/callable wire records retain field/tag order and bounded readers.
Numeric semantic-slot map keys are unchanged, so one focused pair of map helpers
handles their type values; duplicate keys remain rejected by the bounded reader.
Nominal associated-output wire ordering still moves each key with its paired type.
Shared Host record conversion now accepts the enclosing cancellation token and
polls records and flat value nodes. HostInterfaceError gains Cancelled, and its
conversion to DefinitionMappingError preserves cancellation/limit errors. KBC's
required-Host fingerprint uses that shared error conversion. This hidden codec
signature and exhaustive public error matching are Rust API changes; unpublished
format/ABI identifiers remain unchanged.

Focused local validation and selection:

- `cargo build -p kagari-compiler -p kagari-stdlib --lib --message-format=short`
  passes for the source-to-MIR and Native declaration producers used by the selected
  codec scenarios. No compiler library-test target was rerun: its recorded fixture
  migration failures are unchanged.
- A temporary source-free public-API probe reconstructs the existing scoped MIR
  scenario: create an explicit context and I32 function returning 42, verify it,
  round-trip its encoding, lower both versions to bytecode and compare portable
  facts. It confirms retained snapshots reject later/foreign IDs, explicit adoption
  works, editing a terminator loses verification, Native input correspondence rejects
  a different returned constant, and cancellation is preserved. All checks pass.
- A second temporary probe reuses the existing compiler codec/source-identity
  scenarios: checked scalar branch, u64, negative-zero, loop, captured closure and
  enum Hash with user PartialEq/Eq/Hash implementations. Encode before releasing
  source snapshots, decode in an independent scope and compare canonical MIR and
  lowered bytecode. Compare the complete new envelope against a temporary encoding
  of the prior portable semantic records, including every field/tag/table byte.
  For the closure program, also reject truncated/trailing data, an impossible
  module count, a u32::MAX file reference and an out-of-range jump; the latter must
  reach and fail executable verification. All pass. These probes provide local
  behavioral evidence while the retained compiler test owner is being migrated;
  they do not replace that owner or justify keeping the old representation APIs.
- `cargo test -p kagari-contract --test public_abi_wire --message-format=short
  --no-fail-fast`: both shared declaration wire cases pass.
- `cargo test -p kagari-types --lib host_interface:: --message-format=short
  --no-fail-fast`: all 17 selected Host contract/wire cases pass. After extending
  the existing forged-reference case to check cancellation through the exposed
  Host type record encode/decode API, its `host_interface::wire::` filter also passes.
- `cargo test -p kagari-bytecode --test identity
  required_host_fingerprint_is_derived_and_independent_of_docs_and_order
  --message-format=short --no-fail-fast`: the directly affected fingerprint case passes.
- `cargo clippy -p kagari-types -p kagari-contract -p kagari-mir
  -p kagari-bytecode --lib --message-format=short -- -D warnings` passes.

Initial compilation found two non-Copy enum fields in newly authored conversion
records and an unnecessarily exposed nested callable variant; these were repaired
with ordinary cloning and an opaque enum wrapper. Numeric map helper/import cleanup
resolved the remaining local lint findings. No production validation was disabled.
Manual review covers imports, visibility, all serialized fields/tags, paired binding
ordering, bounded readers, retained snapshot ownership and cancellation/error flow.
Structure checks 1079 Rust files with zero violations/exceptions; formatting and
diff checks pass. No full workspace/backend matrix or performance measurement ran.
KBC aggregate wire conversion, shared semantic representation-parameter removal,
obsolete rebinding/path APIs and the recorded compiler/runtime fixture repairs
remain SI05/SI06 work. This checkpoint does not complete SI integration.


### SI05 complete KBC wire aggregate and fingerprint path

The KBC owner now has fixed envelope, program/module, function, instruction,
interface-table and call/field wire records. They compose the shared Native, trait,
layout, semantic-slot, type and Host records introduced by the preceding boundary.
TraitContractRecord and the HostInterfaceRecord decoder are exposed only through
their codec modules for these actual cross-crate consumers; all KBC wire modules,
fields and conversions remain private to the artifact owner. No re-exports or
representation-generic conversion API were introduced.

KbcArtifact read/write, program/artifact encoded-size admission and the three
executable fingerprint entrypoints all use these same fixed records. The private
of_executable_record rebinding helper and portable_projection wrapper are removed.
They no longer clone whole semantic programs/envelopes to construct a second
semantic model instantiated with file references. Canonical reference collection
validates every supplied ID against the retained prefix through DefinitionEncoding
before implicit declarations are collected by verification. Decoding validates the
file table, builds fixed-ID unverified facts and retains their table. Existing
header/ABI, resource, fingerprint, graph, loader and executable sealing steps remain
separate; serialized bytecode_verified and other summaries are still claims to
check, not executable authority. Field/tag order, domains and format IDs are unchanged.

Focused local evidence:

- `cargo test -p kagari-bytecode --test identity --message-format=short
  --no-fail-fast`: all six boundary cases pass, covering malformed envelopes,
  in-memory resource admission, changed content/ABI/graph rejection, Host ABI
  canonicalization and portable path records.
- `cargo test -p kagari-runtime --test offline_host --message-format=short
  --no-fail-fast`: all eight cases pass. These exercise actual source-free artifact
  encoding/loading, independent registration scopes, bound-handle ownership,
  representation/borrowing/effect checks, failed-publication atomicity and reload
  binding compatibility. Callbacks are not executed by offline validation/linking.
- A temporary public-API probe reuses the captured-closure and derived enum Hash
  scenarios from the source-identity checks (including user PartialEq/Eq/Hash
  implementations). For each lowered program, compare new program/module/artifact
  fingerprints with the prior canonical portable-record hash; compare complete KBC
  bytes with the prior encoding; decode into an independent scope, encode again,
  explicitly adopt into a new context, fully verify the artifact and compare the
  executable portable facts. All comparisons and verification pass, including enum
  variant reordering and independent compilation. The probe is migration evidence,
  not a retained consumer justifying the old semantic representations.
- `cargo clippy -p kagari-types -p kagari-contract -p kagari-bytecode --lib
  --message-format=short -- -D warnings` passes. The initial new conversion code
  dereferenced a decoded boxed-call result rather than its wire input; ordinary
  owned-box conversion repaired both cases before running the selected tests.

Manual review covers shared ownership, private wire visibility, bounded readers,
all copied fields/tags, ordered interface/operand vectors, cancellation, error
propagation and the preserved validation sequence. Structure checks 1086 Rust files
with zero violations/exceptions; formatting and diff checks pass. No workspace or
backend matrix, artifact regeneration or performance measurement ran. Both complete
executable codecs now avoid representation rebinding; shared semantic model
parameters, old generic Serde/mapping/path APIs and recorded compiler/runtime test
fixture repairs remain SI06 work. The plan stays active and CI acceptance is unrun.


### SI06 fixed executable models and native correspondence

MIR and bytecode/artifact semantic records and their buffer aliases no longer have
an identity representation parameter. Direct Serde derives/attributes are removed
from identity-bearing executable records; their already established private wire
owners retain all bounded readers. Scalar instruction values, layout slots and
identity-free envelope/debug data retain their existing codecs. There are no
compatibility aliases or alternative path/file-reference executable models.

The executable mapping implementations now explicitly implement DefinitionReferences
and RemapDefinitions. Every reference-bearing field is visited and ID-to-ID remapped
with the previous cancellation/count behavior; they cannot rebind to paths or file
references. Vec implements these fixed capabilities directly, replacing its generic
DefinitionRecord implementation. Editable extraction, scope-prefix validation before
implicit-name collection and explicit cross-context import use these capabilities.
Compiler specialization/lowering, runtime, VM and codegen consumers use the fixed
model types. Removing the generic Default bounds also permits equivalent ordinary
Default derives for bytecode module/function metadata.

The remaining production Native comparison projections are removed.
VerifiedBytecodeProgram no longer exposes portable_projection, and compiler native
input verification does not serialize a semantic file-reference model. The KBC
owner's fixed encode_program_facts entrypoint bounds counts/bytes, validates the
supplied scope and preserves cancellation before returning complete canonical bytes.
Both the sealed expected program and fresh MIR lowering use that exact wire path.
The compiler compares all bytes, preserving floating-point payloads, origins,
concrete identities and imports, before returning the verified MIR seal. Moving
this conversion removes the separate compiler serializer and repeated preparation
of the lowered comparison program; no performance claim is made.

The existing bytecode identity fixture now corrupts a file reference by decoding
only the identity-free envelope prefix, asserting the fixture's module/dependency/
path counts and changing the located four-byte path root. It no longer instantiates
KbcArtifact with file references. The same case additionally checks canonical program
facts after independent-scope adoption and rejects cancelled encoding. All prior
identity, scope, source-free and forged-reference assertions remain.

Focused local evidence:

- `cargo test -p kagari-bytecode --test identity --message-format=short
  --no-fail-fast`: all six cases pass with the migrated corruption fixture.
- `cargo test -p kagari-runtime --test offline_host --message-format=short
  --no-fail-fast`: all eight cases pass after fixed executable consumer migration,
  including source-free loading and rejected reload/publication behavior.
- Scoped library Clippy/fix checks cover common, MIR, bytecode, compiler, runtime,
  VM, codegen and Cranelift, the directly changed executable chain. These are library
  targets only, not package-wide test/feature sweeps. The remaining unused bytecode
  trait import is removed; the final `cargo clippy -p kagari-bytecode --lib --test
  identity --message-format=short -- -D warnings` passes. Earlier failed scoped
  builds identified two leftover MIR generic annotations/visitor calls and a
  Cancelled-to-string assumption; all are repaired before behavioral validation.
- Reused temporary probes now call encode_program_facts instead of the removed
  executable projections. The source-free I32/42 scenario confirms MIR/bytecode
  scope retention, canonical correspondence, foreign/later-ID rejection, explicit
  adoption, seal invalidation on malformed edits, cancellation and rejection when
  a different bytecode constant is supplied as Native input. The source scenarios
  confirm captured-closure snapshot retention, source release before decoding,
  canonical re-encoding/lowering and enum Hash identity under independent
  compilation/variant reordering. Both probes pass against the rebuilt libraries.

Manual review confirms every mapping field, scalar-copy behavior, imports,
verification ownership, fixed-ID signatures, private wire records and canonical
comparison. Structure checks 1086 Rust files with zero violations/exceptions;
formatting and diff checks pass. Shared type/declaration/Host/contract parameters,
their generic Serde/mapping implementations, DefinitionReference and obsolete
metadata path/wire projection APIs still require deletion. Compiler/runtime
library-test fixture failures remain open; the old canonical test fixtures also
still reference removed executable projections and must migrate with their owning
SI06 test cleanup. No unchanged failing library-test target was rerun or counted as
passing. Full backend/feature acceptance remains CI work; the goal is not complete.


### SI06 fixed shared contract models

All identity-bearing kagari-contract models now hold DefinitionId directly,
including callable applications/witnesses, interface normalization and signatures,
layouts, Native imports/result adapters, public items, trait/module contracts and
semantic slots. Their identity representation parameters and direct semantic
Serde implementations are removed. Existing private/shared wire records still own
serialization; canonical encoding, fingerprints and validation do not change.

Each contract implements fixed reference visitation and ID-to-ID remapping,
preserving every field, cancellation and collection limit. ProofCatalog reference
validation consumes DefinitionReferences directly. Physical representation and
string-iteration helpers accept scoped semantic types. Compiler, MIR, bytecode and
runtime consumers use the fixed contract types; no compatibility aliases remain.
Shared kagari-types fields still use their scoped instantiations pending the next
owner migration. Searches find no DefinitionReference, DefinitionRecord, Rebind,
map_identities or visit_definitions in kagari-contract production source.

Focused local evidence:

- Compiler, runtime, VM and Cranelift library checks pass. Scoped Clippy/fix for
  contract, MIR, bytecode, compiler and runtime libraries repairs stale imports;
  the final contract library/public_abi_wire Clippy check passes with warnings
  denied. These are directly affected library targets, not package-wide sweeps.
- `cargo test -p kagari-contract --test public_abi_wire --message-format=short
  --no-fail-fast`: both ABI canonicalization/source-free cases pass.
- `cargo test -p kagari-runtime --test offline_host --message-format=short
  --no-fail-fast`: all eight cases pass, preserving source-free adoption, host
  contracts, failed-publication atomicity and reload binding checks.

- Reused temporary source-free and source probes pass against rebuilt libraries:
  MIR/bytecode scope retention, canonical Native correspondence, mismatched Native
  input, foreign/later references, explicit adoption, cancellation, closure
  snapshots, source release and enum Hash identity/ordering remain covered.

Manual review checks semantic fields against their previous mapping and wire
owners, fixed signatures, imports and visibility. Structure checks 1086 Rust files
with zero violations/exceptions; formatting and diff checks pass. Shared type/declaration/Host
parameters and obsolete common rebinding/projection APIs remain SI06 work, as do
the recorded compiler/runtime/contract and canonical library-test fixture repairs.
No unchanged failing library-test target is counted as passing. Full feature and
backend acceptance remains CI work; the goal is not complete.


### SI06 fixed shared types, Native declarations and Host contracts

All shared kagari-types semantic models now contain DefinitionId directly: Ty,
NominalTy, binders/constraints, Native declarations and modules, callable selectors
and signatures, Host interfaces, value types, members and paths. Their identity
representation parameters and direct semantic Serde implementations are removed.
Explicit DefinitionReferences and RemapDefinitions implementations preserve every
field, unique key, count check and cancellation point. Type conversion/reference
collection helpers are private; external consumers use the fixed capabilities.
Production compiler/HIR, contract, executable, runtime/VM, codegen and standard
library consumers use the fixed models, including foundation partition/adoption.

The bounded flat type and Host wire records retain their shared shape traversal;
private node helpers remain codec-local and do not rebind any semantic model.
Unconditional ID path-limit checks and the unreachable authoring-path branches are
removed. Table construction/decoding still bounds paths; Host validate_in still
checks every scoped nominal leaf, constructor kind, exact Option/Result identity
and collection access. Depth, node counts, hash-key rules and canonical associated
key/value pairing remain in their established owners. No process-local ID gains a
Serde implementation. Canonical bytes and version identifiers are unchanged.

Enum Hash tags now encode the existing fixed NominalTyRecord instead of rebinding
the semantic nominal type to file references. This preserves the table, nominal
arguments/bindings and variant identity tag fields. Host path-only Option/Result
constructors and unused language constructors are removed. The remaining Option/
Result helpers and integer-method result query require a retained table and report
a missing core declaration with None, rather than creating a path-valued type.

Shared type fixtures register directly in a context and compare complete records
after explicit import, replacing whole-model path projections. Canonical ordering
still exercises reversed allocation, associated key/value pairing and unrelated
identities. The duplicate generic-Serde composite roundtrip is consolidated into
the complete PortableTypes identity case; malformed flat nodes and noncanonical
bindings remain at the wire owner. Host metadata integration uses real KHI bytes;
its duplicate file-reference/kind corruption cases are covered by the already
selected private-wire and scoped Host validation cases. Foreign metadata,
cancellation, repeated adoption and retained ownership assertions remain.

Focused local evidence:

- Scoped library Clippy/fix covers types, contract, HIR, MIR, bytecode, compiler,
  runtime, VM and Cranelift. Standard-library partition's remaining old visitor/
  remapper calls surfaced during offline Host compilation and were migrated; its
  library Clippy check then passed. These select directly affected library targets
  without package-wide or backend/feature sweeps.
- `cargo test -p kagari-types --lib host_interface:: --no-fail-fast
  --message-format=short`: all 17 Host boundary/codec cases pass.
- `cargo test -p kagari-types --lib ty:: --no-fail-fast --message-format=short`:
  all 16 type identity, wire, access and substitution cases pass.
- `cargo test -p kagari-types --lib declaration::verify::scoped_tests::
  --no-fail-fast --message-format=short`: all four scoped declaration cases pass.
- `cargo test -p kagari-types --test host_metadata --no-fail-fast
  --message-format=short`: both explicit metadata/KHI adoption cases pass.
- `cargo test -p kagari-runtime --test offline_host --no-fail-fast
  --message-format=short`: all eight cases pass against rebuilt dependencies.
- Final `cargo clippy -p kagari-types --lib --tests --message-format=short --
  -D warnings` passes after repairing the owner's old fixtures and stale imports.
  The initial library-test compilation reported 43 path/generic-Serde fixture
  errors and the metadata integration compilation reported 20 obsolete projection
  errors; all are resolved before the reported behavioral checks.

- Reused temporary public-API probes pass against rebuilt libraries. Source-free
  MIR/bytecode correspondence, foreign/later-reference rejection, explicit adoption,
  Native mismatch and cancellation are preserved. Captured-closure snapshots,
  source release before decoding and canonical enum Hash tags under independent
  compilation/variant reordering also pass.

Manual review checks semantic field coverage, same-ID remapping, codec ownership,
associated binding order, ID Copy changes, imports and visibility. Structure checks
1086 Rust files with zero violations/exceptions; formatting and diff checks pass.
Common DefinitionReference/DefinitionRecord/Rebind, metadata path/portable projections and
legacy query helpers still require removal. Recorded compiler/runtime/contract
library-test and remaining example/integration fixtures remain open SI06 work.
Full backend/feature acceptance is reserved for CI; the goal is not complete.


### SI06 delete common rebinding and complete SDK scope adoption

Removed DefinitionReference/DefinitionDescription and their module, DefinitionRecord/
Rebind, blanket semantic adapters, DefinitionMetadata::to_paths/to_portable,
scope_record and PortableMetadata. DefinitionMapper accepts only DefinitionId to
DefinitionId and preserves cancellation/error propagation; all semantic owner
signatures use this fixed mapper. Metadata retains scope validation and explicit
imports without gaining semantic or executable authority.

Protocol::from_definition requires a retained table, Host nominal validation has
one scoped entrypoint, and exact builtin path queries compare against table lookup.
The declaration verifier directly checks callable namespace/kind facts previously
read through DefinitionDescription. Runtime installed-trait checks use module/name
queries and scoped catalog keys without materializing paths. Every production
caller across HIR, compiler, executable and runtime layers uses the fixed queries.
Bytecode count/type checks remove the old always-true ID path predicates and empty
Host identity walk. Actual ModuleIdentity/path-table checks, type depth/node limits,
collection counts, Host semantic validation and complete reference-prefix checks
remain with their established owners.

SDK checking exposed three previously unmigrated production calls: Native module
export still treated scoped extraction as fallible; artifact emission returned a
bare record; preparation verified a bare record without its scope. The export now
retains metadata directly. Emission returns DefinitionMetadata<BytecodeArtifact>
after explicit adoption and artifact construction. Preparation resource-checks the
input before adoption, then performs complete artifact/native correspondence
verification in its context; identity cancellation retains the cancellation error.
The existing source_modules and scoped_execution examples use retained scopes and
explicit import for editable products. HIR body lookup takes a scoped ID, and source
display borrows the owning table instead of projecting a whole type to paths.

The first source_modules run found a runtime construction defect: NativeInstallation
created a default Runtime/Host context, then replaced only the Native registry.
Host linking correctly rejected the two contexts. Runtime now constructs both
registries from the final Native registry through a private constructor; ordinary
Runtime::new supplies its default registry through the same path. No validation or
Host owner check is bypassed. The prepared installation still shares registration
records/name allocation while heaps, Host registry owners, loaded generations and
later registration publication remain runtime-local. The example now loads the
same prepared program in two runtimes, verifies both return 42 and rejects a loaded
module from the other runtime.

Focused local evidence:

- Compiler/runtime/SDK library checks pass; the three SDK API errors are repaired.
  Common/types/bytecode/SDK library Clippy passes with warnings denied. Final SDK
  library plus source_modules/scoped_execution example Clippy also passes.
- `cargo test -p kagari-types --lib declaration::verify::scoped_tests::
  --no-fail-fast --message-format=short`: four scoped declaration cases pass. The
  initially combined language_identity target matched no filter and is not counted;
  its separate unfiltered target run passes its one exact protocol/snapshot case.
- `cargo test -p kagari-bytecode --test identity --no-fail-fast
  --message-format=short`: six resource, canonical identity and forged-input cases
  pass with the actual path/table and bounded codec checks retained.
- `cargo test -p kagari-runtime --test host_registration --no-fail-fast
  --message-format=short`: six cases pass after explicit fixture registration and
  scope ownership migration, preserving duplicate publication, borrow cleanup,
  borrowed-result rejection and stable runtime metadata assertions.
- `cargo run -p kagari-embed --example source_modules --message-format=short`:
  source queries, independent KBC decode, imported public type ownership and
  execution of 36 linked modules pass. The final two-runtime assertions pass
  after the NativeInstallation construction repair.
- `cargo run -p kagari-embed --example scoped_execution --message-format=short`:
  SDK compilation with portable MIR, artifact preparation, repeated execution,
  cancellation/reuse, missing entry and ambiguous entry rejection pass.

Manual review covers deletion consumers, scoped mapper fields, query predicates,
retained input resource checks, artifact/native verification order, installation
ownership and preserved Host/generation checks. Remaining legacy compiler fixture
helpers and runtime host_nominal/offline_types still reference removed APIs and
are not buildable yet; they are owned by the next SI06 fixture cleanup. Previously
recorded compiler/runtime/contract library-test repairs and further SDK examples/
integration tests remain open; unchanged failing targets were not rerun. No local
workspace/feature/backend matrix or performance measurement ran. CI acceptance and
final plan retirement remain outstanding; the goal is not complete.

Final checkpoint checks: `uv run --locked scripts/check_structure.py` checks 1085
Rust files with zero violations/exceptions; `cargo fmt --all -- --check` and
`git diff --check` pass.


### SI06 CLI and retained Host fixture scopes

CLI emission encodes the artifact records with their retained definition table;
its execution helper takes scoped artifact metadata through preparation. The
existing emit/decode/run pipeline test passes. No CLI command or execution policy
changes.

Migrated runtime host_nominal and offline_types fixtures to explicit registration
contexts and fixed-ID semantic records, removing their obsolete scope_record and
DefinitionRecord/Rebind helpers. Offline decoding retains its independent scope;
comparisons import into a common scope, while runtime publication still performs
its own adoption. Wrong-kind identity tests use a registered module root rather
than mutating a path stored in a semantic field. Name-equivalent definitions in
different modules, cross-runtime root/path rejection, failed batch publication,
mutual references, required method callbacks, portable Host ABI fingerprints and
contract-change rejection keep their original assertions. No production semantic
checks or tests were removed.

Focused local validation:

- `cargo test -p kagari-cli --bin kagari-cli
  emits_and_runs_kbc_artifacts_through_cli_pipeline --no-fail-fast
  --message-format=short`: one existing pipeline case passes.
- `cargo test -p kagari-runtime --test offline_types --no-fail-fast
  --message-format=short`: all four selected offline type cases pass.
- `cargo test -p kagari-runtime --test host_nominal --no-fail-fast
  --message-format=short`: all eight selected nominal/path boundary cases pass.
- Host fixture Clippy passes with warnings denied. Initial host_nominal build
  found five view API/borrow mismatches during migration; those are repaired and
  its selected scope subsequently passes. Previously recorded unrelated library
  fixture failures were not rerun.

The remaining references to removed common representation APIs are in compiler
codec/ABI tests. Broader old constructor, verifier and artifact consumers in
compiler/runtime/contract library tests and SDK examples still require migration.
This checkpoint closes the two carried runtime integration fixture targets only;
exhaustive feature/backend validation remains CI scope, and SI06 is not complete.

CLI binary Clippy also passes with warnings denied. Manual review confirms retained
source/decoded/runtime scope separation and equivalent malformed identity cases.
Structure checks 1085 Rust files with zero violations/exceptions; formatting and
diff checks pass. No new production exports, generic identity models or semantic
path projections were introduced.


### SI06 contract matching and verifier fixture migration

Matching fixtures now import the installed collection contract into an explicit
context and construct custom nominal applications from registered IDs. Repeated
binders and associated family members keep distinct registered owners. Verifier
fixtures likewise register source declarations before validating them, import
installed conversion adapters, and construct forged method names/kinds through
context allocation. A registered wrong-kind member still reaches contract
verification; registration does not authorize a forged adapter. Method-substitution,
linked projection and cancellation assertions remain intact.

`cargo test -p kagari-contract --lib types::matching::tests:: --no-fail-fast
--message-format=short` was selected for the changed matching behavior, but the
library test executable cannot yet build. The initial diagnostic set had 221
errors; after migrating matching/verify fixtures and fixing one definition-query
argument order, the final attempt reports 165 errors, none in these two changed
files. Representative remaining failures are missing DefinitionPath fixture types
in applications/proofs, old ModuleDecl::definition/variant_id signatures in callable
interface tests, missing retained tables in ProofCatalog::new and trait_contract,
and path mutation on fixed IDs in inheritance/native tests. The owning SI06
follow-up is migration of those existing library fixture files; no selected test
ran, and no pass is claimed. Do not repeat this command before repairing those
consumers. The next behavioral selection should include the changed verifier cases
once the library test executable builds, then the directly changed fixture contracts;
complete workspace/backend coverage remains CI-only.

Manual review confirms fixed owner/member IDs, retained scope access and unchanged
rejection assertions. `uv run --locked scripts/check_structure.py` checks 1085 Rust
files with zero violations/exceptions; `cargo fmt --all -- --check` and
`git diff --check` pass. This intermediate checkpoint changes test fixtures only;
previous production/library and focused integration evidence remains applicable.
The goal and final documentation retirement remain open.


### SI06 complete contract fixture scope migration

Migrated the remaining contract library fixtures (applications, native layouts,
inheritance, proof catalogs and interface calls) plus instantiation/native-default
integration fixtures. Each fixture registers IDs before constructing semantic
records and retains an explicit context/table for queries and validation. Protocol
proof fixtures register intrinsic names only; no new carried trait contract or
implementation is supplied to make a negative proof pass. Forged names, kinds,
occurrences and module owners remain distinct registered IDs and preserve all
existing contract rejection assertions. Native default instantiation compares with
the original table in the same scope, rather than comparing unrelated fresh IDs.

The 165 carried contract library compilation errors are resolved. Clippy also
exposed 67 old-API errors in the two integration fixtures, which are now migrated.
Three runtime test failures during migration came from constructing query IDs after
the ProofCatalog snapshot. The fixtures now register those query names before
publishing the immutable catalog, preserving snapshot-prefix validation. No
production identity check was relaxed and no new compatibility interface was added.

Removed direct serde roundtrip assertions on NativeDefaultApplication, which is
now exclusively semantic data. Its actual independent-scope roundtrip remains in
public_abi_wire's existing NativeDefault-bearing public-item coverage. The distinct
4097-argument decoder rejection moved to that codec owner's bounded-reader test,
which now rejects the real NativeDefault wire prefix before reading any elements.
Transformation limit and cancellation assertions remain in native_defaults.

Focused local evidence (only affected contract scopes):

- `cargo test -p kagari-contract --lib types:: --no-fail-fast
  --message-format=short`: 25 cases initially passed and two snapshot-order fixture
  failures were repaired. The two failed proof cases were rerun by exact name and
  both pass; all 27 selected type cases have passing evidence. This also completes
  the previous checkpoint's skipped matching/verifier checks.
- `cargo test -p kagari-contract --lib callable::interface::tests::
  --no-fail-fast --message-format=short`: all six interface cases pass.
- `cargo test -p kagari-contract --test instantiation --test native_defaults
  --no-fail-fast --message-format=short`: instantiation and 12 default cases passed;
  the remaining snapshot-order case was repaired and its exact-name rerun passes.
- `cargo test -p kagari-contract --test public_abi_wire --no-fail-fast
  --message-format=short`: both independent-scope roundtrip and bounded wire-reader
  cases pass, including the moved default argument limit.
- `cargo clippy -p kagari-contract --lib --tests --message-format=short -- -D warnings`
  passes after repairing the identified fixtures and redundant Copy/field syntax.
  Manual review checks owner/member correspondence, source-free proof assumptions,
  scope capture order, fixed-ID query helpers and preservation of negative cases.
- Structure checks 1085 Rust files with zero violations/exceptions; formatting and
  diff checks pass. No workspace/backend/feature test matrix ran locally.

Compiler/runtime library fixtures and remaining SDK/VM consumers still require
SI06 cleanup. This checkpoint resolves contract fixture build failures, not final
cross-subsystem acceptance; final CI scope and plan retirement remain outstanding.


### SI06 compiler fixture ownership entrypoints

Compiler test helpers now unwrap the fallible scoped foundation catalog, retain
DefinitionMetadata when returning lowered bytecode and explicitly import edited
MIR roots and the original dependency closure into a common verification context.
The shared helper revalidates the whole graph before lowering; it does not discard
scope evidence or restore bare path records. Source ABI fixtures query protocols
and associated members through the analyzed table, validate Native/public records
with that same retained scope, and remove scope_record from the ABI test owner.

`cargo test -p kagari-compiler --lib source::lower::abi::tests:: --no-fail-fast
--message-format=short` remains a carried build failure, now reporting 839 errors
and one unused-import warning across the old compiler consumers; no selected test
ran. There are no diagnostics in the two changed files. The larger diagnostic set
reflects the fixed helper return types exposing callers' direct field access and
the completed shared model API removals, not passing integration acceptance.
Representative counts are bytecode identities 122, interfaces 111, lowering 87,
MIR verify 83, bytecode validation 80, Host contracts 75 and artifacts 65. Their
SI06 follow-up must retain scope while editing/revalidating records, update exact-ID
assertions and use codec-owned wire facts instead of semantic serde. The compiler
codec test is now the only remaining scope_record/to_portable consumer. Its forged
MIR codec cases need an owner-appropriate fixture migration without exposing a new
unverified production encoder or weakening decoder/analysis-budget assertions.
Do not repeat unchanged library build failures before repairing those consumers;
complete the selected ABI cases once the library test target builds.

Manual review covers table provenance, identity Copy changes and graph revalidation.
Structure checks 1085 Rust files with zero violations/exceptions; formatting and
diff checks pass. Previous contract and production checks remain evidence for
unchanged code; full-workspace/backend checks and final cleanup remain outstanding.


### SI06 compiler and MIR fixture migration

Migrated all remaining compiler library fixtures and the source_programs integration
fixture to retained DefinitionContext/DefinitionMetadata ownership. Editable MIR
records are explicitly adopted, mutated and reverified with their current table;
bytecode fixtures collect implicit ABI names through the existing identity owner
before source-free verification. Manual modules register their roots. Single-ID
fault injection interns an altered authoring path without converting semantic
records back to a path representation. Public ABI, specialization, imported method,
Host trait and executable-layout assertions continue to compare IDs in one scope.
Independent codec comparisons use canonical file facts and PortableTypes instead
of serializing semantic models. The last scope_record/to_portable consumers are
removed; a repository crate search finds neither obsolete entrypoint.

Moved the forged control-flow and whole-program analysis-budget decoder cases to
kagari-mir's private codec wire test owner. They build real file records from
source-free MIR, first establish valid baseline verification, and retain the
specific InvalidBlock and cumulative analysis-state-limit rejection assertions.
The production encoder still accepts only VerifiedMirProgram. No new unverified
public encoder, identity compatibility model or verifier relaxation was introduced.

The carried compiler library compilation failures are resolved. Initial execution
found seven bytecode fixture setup failures: missing roots on manually constructed
modules and missing implicit-name collection on one imported lowering result.
All seven were repaired without changing their semantic assertions. Clippy also
exposed 39 old-API errors in source_programs and Copy/slice lint findings in migrated
unit fixtures; these are repaired. A discarded attempt to restrict Clippy by passing
`--test` as a rustc flag affected dependency builds and failed crate resolution;
the standard package-scoped command below succeeds and is the validation evidence.

Focused local evidence (103 distinct selected tests, no workspace/backend sweep):

- `cargo test -p kagari-compiler --lib tests::bytecode:: --no-fail-fast
  --message-format=short`: 56 initially passed; exact-name reruns of all seven
  repaired cases pass, completing all 63 selected cases.
- Compiler library selection: all seven codec cases, three ABI contract cases,
  six visibility cases, two interface-signature cases, the async MIR case, three
  origin cases, program-points case, four source ABI cases, edited-root and analysis
  storage-limit cases, scalar-folding and non-SSA pass cases, and applied-nominal,
  enum-layout and cancelled-verification cases. All 34 pass. This completes the
  previously blocked source ABI selection while covering scope adoption, canonical
  codecs, snapshot ownership, reanalysis and rejection boundaries.
- `cargo test -p kagari-mir --lib codec::wire::tests:: --no-fail-fast
  --message-format=short`: both moved decoder rejection cases pass.
- source_programs selected by the exact names imported_generic_methods_have_distinct_program_instances_and_share_the_limit,
  missing_dependencies_and_mismatched_link_signatures_are_rejected,
  public_abi_distinguishes_same_named_imported_types_and_constraints, and
  public_generic_abi_ignores_binder_spelling_and_constraint_source_order: all four pass.
- `cargo clippy -p kagari-compiler -p kagari-mir --lib --tests
  --message-format=short -- -D warnings` passes. Manual review includes fixture
  table lifetime, registration-before-snapshot ordering, negative assertions,
  cancellation and wire-owner access. Imports in changed compiler unit fixtures
  are consolidated at module scope; globs remain confined to test modules.
- Structure checks 1086 Rust files with zero violations/exceptions; formatting and
  diff checks pass. Unchanged previously passing production evidence remains valid.

Runtime/SDK/VM consumers and bytecode codec fixtures remain SI06 work, as do the
final architecture/specification documentation transfer and plan retirement.
Exhaustive workspace, feature/backend and CI acceptance remains unrun locally.


### SI06 bytecode codec and verification fixtures

Migrated canonical artifact, linked associated-type/family-bound and async bytecode
fixtures to explicit DefinitionContext ownership. Artifact creation, hashing,
encoding and loader validation now receive the retained scope; decoded metadata is
explicitly imported before use. Tests of oversized wire collections use the codec
owner's real ArtifactEnvelope conversion, replacing portable_projection and direct
semantic instruction serialization. There is no public unverified encoder addition.

Oversized Host identity paths are now unrepresentable as semantic IDs. The existing
path-limit fixture therefore asserts PathLimit at root/definition registration and
retains the separate module-record/header artifact preflight checks. Invalid Host
type fixtures retain direct semantic verifier rejection and now assert the precise
Identity(InvalidContract) returned by bounded reference traversal before artifact
encoding/fingerprinting. The initial failed expectation assumed this work still
reached the later bytecode-verification stage; no validation was removed or relaxed.
Protocol test setup registers only intrinsic names; declared Hash and associated
contracts remain the evidence used by the linked-bound verifier.

Focused local evidence:

- `cargo test -p kagari-bytecode --lib artifact::canonical_tests:: --no-fail-fast
  --message-format=short`: 20 passed initially; the invalid Host type case's repaired
  exact-name rerun passes, completing all 21 selected codec/resource cases.
- `cargo test -p kagari-bytecode --lib --no-fail-fast --message-format=short --
  trait_bounds::tests:: verifier::async_tests::`: all five cases pass, preserving
  corrupted associated outputs, family input assumptions, suspension liveness,
  Host capability rejection and source-free async artifact validation.
- `cargo clippy -p kagari-bytecode --lib --tests --message-format=short --
  -D warnings` passes after one unnecessary-reference fixture repair.
- Structure checks 1086 Rust files with zero violations/exceptions; formatting and
  diff checks pass. Manual review covers actual wire records, preflight order,
  explicit adoption, registration versus semantic proof and test-only glob scopes.

Runtime/SDK/VM test consumers, final contract-document transfer and plan retirement
remain outstanding. This fixture checkpoint does not claim unrun workspace CI.


### SI06 runtime fixture ownership entrypoints

Shared runtime layout fixtures now register fixed IDs in an explicit authoring
context and return scoped programs to Runtime loading. Nominal input types are
validated against the runtime's published HostRegistry snapshot and imported into
that authoring context; private runtime writers are not exposed to integration
fixtures. Native sequence fixtures retain their module declaration metadata and
use the current fallible builders. The gc_ownership target reuses the shared
allocation-owner fixture and scopes its retained Host callback declaration.

Migrated the runtime staging, retention, module-state, GC metadata-root and frame
window fixture entrypoints to DefinitionMetadata. Module-record fixtures retain
lowered metadata through reload, explicitly split/recombine the scope for edited
layout facts, and inspect detached records through their envelope. Native method
application fixtures use fallible builders, Exact(Type) codecs and fixed type
argument resolution. These changes preserve runtime-owned normalization and leave
runtime publication/verification logic unchanged.

Focused local evidence and carried errors:

- `cargo test -p kagari-runtime --test gc_ownership --no-fail-fast
  --message-format=short --
  foreign_handles_and_wrong_value_tags_are_rejected_before_mutation_or_accounting
  shared_host_callbacks_can_retain_checked_root_leases`: both pass. This covers
  the shared nominal/enum allocation fixture across independent runtimes plus Host
  callback roots retained across collection. Four unrelated target cases remain
  outside this local selection.
- `cargo clippy -p kagari-runtime --test gc_ownership --message-format=short --
  -D warnings` passes, including compilation of the shared layout helpers.
- `cargo test -p kagari-runtime --lib module::staging::tests:: --no-fail-fast
  --message-format=short` remains a carried build failure. The latest attempt has
  130 errors, down from 175 before this batch; no selected staging test ran. There
  are no diagnostics in the changed unit fixture files or shared layout support.
  Remaining owners are root runtime artifact/reload tests, module snapshot tests,
  Host path registration tests in GC/reflection/value, and old native builder,
  codec and payload consumers. Complete the pending staging selection after those
  consumers build; do not treat the passing integration cases as their evidence.
- Structure checks 1086 Rust files with zero violations/exceptions; formatting and
  diff checks pass. Manual review covers scope provenance, independent runtime
  adoption, snapshot retention and test-only import/visibility boundaries.

The runtime unit build, remaining SDK/VM consumers and final documentation cleanup
remain required SI06 work. Full-workspace/backend validation remains reserved for CI.


### SI06 runtime unit snapshot and publication fixtures

Completed the remaining runtime library-test callers of scoped registration,
verification, artifact loading and reload. Host path fixtures retain independently
authored declarations through registration, then use the runtime's published table
for installed field IDs. Native fixtures explicitly adopt provider declarations and
compare imported catalogs; fixed-ID entries replace path-key iteration. Re-export
fixtures import the complete provider catalog because validating individual standard
modules cannot establish cross-module return-interface contracts. Missing/corrupt
export and uninstalled-provider rejection assertions remain in place.

Shared verified programs, editable detached modules, artifact cache fingerprints
and nominal enum reload fixtures now retain their actual definition scope. Detached
module edits split and recombine the existing envelope rather than assign a new
unrelated table. The module-store test body moved intact into `module/tests.rs`:
explicit scope setup raised the facade above 1200 effective LOC, and this normal
Rust child module restores the implementation/test responsibility boundary without
exceptions or new public entrypoints.

Focused local evidence and remaining errors:

- `cargo test -p kagari-runtime --lib module::staging::tests:: --no-fail-fast
  --message-format=short`: all six previously blocked staging cases pass; the
  runtime library-test build's carried 130 errors are resolved.
- The selected native::module::tests, module::records::tests and
  module::retention::tests groups plus shared verification/resource rejection,
  private runtime linkage, nominal enum reload, Host value categories, reload
  publication/ABI/staleness and cache invalidation selected 36 cases. 35 passed
  initially; the complete-provider fixture repair's exact-name rerun of
  installed_reexports_require_declared_and_installed_canonical_targets passes,
  completing this selection. The filter also selected the existing oversized
  reload preflight case. Other runtime tests remain outside local execution scope.
- `cargo clippy -p kagari-runtime --lib --tests --message-format=short --
  -D warnings` exposed unmigrated integration-test consumers and failed before
  completing all targets. The observed 110 diagnostics belong to enum_declarations,
  installation_access, native_builder, native_conversion, host_objects,
  execution_frames and host_borrows; representative causes are fallible Native
  builders, old Scalar codecs, unscoped program loading and artifact decoding.
  There are no diagnostics in the changed library-test files. This does not imply
  that other integration targets have completed linting. Continue those consumer
  migrations in SI06, then run a small selected boundary set rather than all tests.
- `cargo clippy -p kagari-runtime --lib --message-format=short -- -D warnings`
  passes for the production library.
- Manual review preserves negative assertions, catalog scope provenance, immutable
  retained metadata and runtime-local publication/GC boundaries. Globs remain
  test-only. Structure checks 1087 Rust files with zero violations/exceptions;
  formatting and diff checks pass.

Runtime integration, embed/VM consumers and final contract-document transfer/plan
retirement remain required. No full-workspace, backend matrix or CI pass is claimed.


### SI06 runtime Native and installed-boundary integration consumers

Migrated Native builder, enum declaration and recursive conversion integration
fixtures to fallible registration/type application, Exact(Type) receiver codecs and
retained declaration metadata. Generated documentation tests resolve the same single
declaration in each published table before querying sites; semantic records remain
fixed-ID. Forged enum-provider identities are authored in a retained independent
scope, preserving the original semantic rejection checks. The raw inherent-owner
fixture registers both the valid and forged module roots so the negative case still
exercises receiver ownership rather than an absent identity registration.

Host object fixtures retain compiler output through a real artifact encode/decode,
explicitly adopt decoded records, then verify before runtime loading. Installation
fixtures retain the producer table and import standard declarations before comparing
reserved contracts. Host callback signatures are registered before taking their
metadata snapshot. Suspended execution frame/candidate tests preserve the detached
module's original table; Host borrow tests wrap their identity-free executable input
without changing escape or cleanup assertions.

Focused local evidence and remaining work:

- Three selected integration targets compiled and nine selected cases passed:
  native_builder's ordinary Rust signature, builtin inherent ownership and trait
  associated outputs; enum_declarations' authoring, documented sites, changed-view
  rejection, malformed/foreign applications and missing provider; native_conversion's
  recursive cycle rejection/shared-child acceptance. Other cases in these targets
  remain outside local execution scope.
- Four further targets compiled and nine selected cases passed: installation_access's
  installed Host function, sticky cancellation, storage capabilities and reserved-core
  contracts; execution_frames' two suspended session/candidate isolation cases;
  host_borrows' no-escape/storability case; host_objects' foreign/nominal rejection and
  old-version binding retention after reload. Compilation uses the current codec and
  shared scoped layout fixtures; no complete runtime suite was executed.
- `cargo clippy -p kagari-runtime --test native_builder --test native_conversion
  --test enum_declarations --test installation_access --test execution_frames
  --test host_borrows --test host_objects --message-format=short -- -D warnings`
  passes. This resolves the preceding checkpoint's 110 observed integration errors.
- A subsequent `cargo clippy -p kagari-runtime --lib --tests --message-format=short
  -- -D warnings` reaches further unmigrated targets and fails with 114 observed
  diagnostics: typed_path_views/{main,contracts,mutation}, host_scopes,
  execution_sessions, runtime_substrate and mutation_resources. Representative
  remaining contracts are scoped Host type/function/path registration and scoped
  program loading. This is a carried SI06 consumer build failure; early-exit targets
  are not counted as linted or executed. Repair this batch before attempting the
  same package-wide test-target lint again.
- Structure checks 1087 Rust files with zero violations/exceptions; formatting and
  diff checks pass. Manual review covers fixed-ID record usage, registration before
  snapshots, independent producer/runtime scopes, negative-case causes, normal
  imports and unchanged assertion coverage.

Runtime integration consumers, embed/VM consumers, lasting contract documentation
and plan retirement remain required. Exhaustive tests/features/backends remain CI
scope and have not been run locally.
