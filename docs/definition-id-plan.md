# Definition IDs throughout semantic processing

Status: active implementation; SI01 is complete and SI02 is in progress.
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
