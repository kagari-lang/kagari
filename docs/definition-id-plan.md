# Definition IDs throughout semantic processing

Status: active implementation; SI01 is complete; SI02 and its coupled SI03 migration are in progress.
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
| [Identity references](../crates/kagari-common/src/identity/reference.rs) | `DefinitionReference` abstracts paths, IDs and file references. Portable references cannot satisfy ordinary table lookup/display operations. Remove the shared semantic abstraction. |
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
