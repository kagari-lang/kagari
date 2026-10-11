# Builtin fixed-length arrays (SA20)

Status: BA01-BA05 are implemented and locally accepted. Full GitHub CI remains
open. The
[roadmap](implementation-roadmap.md#builtin-fixed-length-arrays-sa20) owns queue
placement; this file owns phase order, acceptance and the progress ledger.

## Outcome and scope

Implement [SA20](review.md#sa20-separate-builtin-fixed-length-arrays-from-library-lists)
with the agreed length model: `[T]` is a builtin, identity-bearing array whose
length is determined at construction and cannot subsequently change. Length does
not participate in type identity. `[1, 2, 3]` and `[value; count]` construct this
type. `Vec<T>` is a distinct registered library type; List/MutableList remain
library interfaces. Construct collections explicitly with `Vec::from(array)` and
`HashSet::from(array)`, retaining `Vec::new()` and `HashSet::new()` for empty ones.

Implemented behavior:

```kagari
fn first(values: [i32]) -> i32 { values[0] }

fn main() -> i32 {
    val array = [1, 2, 3]; // Inferred as [i32].
    val alias = array;
    alias[0] = 42; // Same object; length remains three.
    val short: [i32] = [7]; // Same type, different object length.
    val values = Vec::from(array);
    values.push(4); // Does not change array or alias.
    val readable: List<i32> = values;
    val unique = HashSet::from([1, 2, 2]);
    first(array)
}
```

The examples assume the usual installed library names. The builtin array type
and literal must not depend on a registered Vec or List declaration. Core trait
implementations still use explicitly supplied, checked protocol declarations.

Reject implicit conversion between Array and either Vec or List, including
`val v: Vec<i32> = [1, 2]`,
`val l: List<i32> = [1, 2]` and passing a Vec to a `[i32]` parameter. Array does
not implement List or MutableList. Array has no push, pop, insert, remove, clear
or other length-changing operation. A `var` binding may be rebound to a different
array; that does not resize the old object or redirect its aliases.

This task includes the affected type, declaration, compiler, runtime, host
conversion, library, tooling and artifact consumers. It does not activate SA18's
general removal of collection access markers, SA19's identity migration or SA21's
host registration unification. Move Vec off the intrinsic Array representation;
leave the independent HashMap/HashSet representation cleanup for later. The new
HashSet constructor is included without requiring that broader migration.

Exclude type-level lengths, const generics, slices, implicit conversions, variance,
structural equality, varargs, macros and a general collection API expansion. Do
not add a dormant optional-length field or a second factory naming scheme. No
performance target or speedup is claimed by this semantic separation.

## Initial implementation and migration owners

This inventory records the pre-migration checkout. Array described growable Vec
storage at several boundaries. Removing methods from the source surface alone
could not enforce fixed length; the migration separated these identities and
operations. The completed representation is recorded in the BA01 decision below.

| Owner | Initial coupling and required work |
| --- | --- |
| [HIR type syntax](../crates/kagari-hir/src/hir/ty.rs), [type resolution](../crates/kagari-hir/src/typeck/ty.rs), [array bridge](../crates/kagari-hir/src/builtin/array_bridge.rs) | Syntax retains only the element type; resolution maps `[T]` to installed List. Resolve it directly to builtin Array and remove List-based literal contextualization. Preserve independent List indexing support where still needed. |
| [HIR semantic types](../crates/kagari-hir/src/types.rs), [shared types](../crates/kagari-types/src/ty.rs), [native application](../crates/kagari-hir/src/native.rs) | `TypeId::Array`/`Ty::Array` carry CollectionAccess; `NativeTypeKind::Vec` maps to that representation. Give Array its own element-only semantic form and use the existing nominal native-storage model for Vec. |
| [Native storage descriptors](../crates/kagari-types/src/declaration/native.rs), [stdlib declarations](../crates/kagari-stdlib/src/catalog/defaults.rs) | Vec currently uses `NativeTypeConstructor::Array`. Register Vec through its nominal declaration and sequence layout, retaining checked ownership and implementation selection. |
| [Literal inference](../crates/kagari-hir/src/typeck/body.rs), [source lowering](../crates/kagari-compiler/src/source/lower/expr/aggregates.rs) | Literals infer the mutable intrinsic representation and emit MakeArray/RepeatArray. Produce checked builtin Array facts without selecting library storage. |
| [Contract operations](../crates/kagari-contract/src/operations.rs), [MIR verifier](../crates/kagari-mir/src/verify/operation.rs), [bytecode verifier](../crates/kagari-bytecode/src/verifier/operation.rs) | Carry and independently verify distinct builtin array and library sequence contracts through lowering, encoding and loading. |
| [GC array allocation](../crates/kagari-runtime/src/gc/arrays.rs), [sequence storage](../crates/kagari-runtime/src/native/sequence.rs), [VM dispatch](../crates/kagari-vm/src/executor/dispatch.rs) | Current allocations use registered sequence payloads and share resize operations with Vec. Separate fixed-length object authority from growable sequence authority while reusing sound storage primitives. |
| [Rust Vec conversions](../crates/kagari-runtime/src/native/conversion/composites.rs), [ScriptVec](../crates/kagari-runtime/src/native/collections/vector.rs) | Both currently accept intrinsic Array. Keep these APIs attached to the registered Vec type; audit array-specific host adapters independently. |
| [Collection construction](../crates/kagari-stdlib/src/catalog/construction_defaults.rs), [list algorithms](../crates/kagari-stdlib/src/catalog/list_methods.rs), [string declarations](../crates/kagari-stdlib/src/catalog/strings.rs) | Existing Array occurrences also describe collection results and algorithm buffers. Classify each producer and consumer by intended contract before changing it. |

Also audit mapping, substitution, type display, source queries/cache invalidation,
native binding requirements, interface witnesses, primitive admission, reflection,
typed paths, runtime value checks and host schemas. These consumers must agree
on the distinction; backend code must consume checked facts rather than infer it
from a source spelling or a Rust payload shape.

## Required semantic and execution contracts

### Array values and construction

- `[T]` has one invariant element type and no length parameter. Arrays of lengths
  zero, one and four have the same type when their element types agree. A typed
  empty literal such as `val a: [i32] = [];` is valid; unresolved element inference
  must still diagnose an error rather than choose an arbitrary type.
- Explicit annotations are optional: `val a = [1, 2, 3];` infers `[i32]` under
  ordinary integer defaulting. Context such as `val a: [u8] = [1, 2, 3];` instead
  checks the elements as u8. Retain ordinary element inference and range checks.
- Array is a shared GC object. Assignment and calls preserve identity; element
  replacement is visible through aliases. Preserve identity equality/hashing and
  existing rooting rules. `val` prevents rebinding, not element replacement.
- Array literals evaluate elements left-to-right, exactly once. Repeat syntax
  evaluates the value and then the `usize` count once each, including when count
  is zero. Count may be computed at runtime. Preserve checked allocation arithmetic,
  completed effects, failure order, cancellation and cleanup.
- Preserve the existing repetition restriction from
  [value semantics](spec/value-semantics.md#repeat-arrays-and-bulk-replacement):
  repeat only types proven free of shared mutable object identity, recursively
  through tuples and enums. This applies at zero and one as well. Explicit
  `[object, object]` remains the way to request shared identities.
- Provide length/is_empty, checked element reads and writes, and iteration. Keep
  current valid integer indexing behavior, bounds traps and once-only place
  evaluation. Registered Index/Iterable implementations use the existing checked
  protocol machinery; builtin indexed assignment must not require MutableList.
  Do not introduce a new general indexing trait merely for this migration.
- Preserve iteration admission and alias-mutation checks at their existing owner.
  Fixed length does not by itself authorize writes during a protected native
  operation or make an iterator independent of its source.

### Collection constructors and existing library behavior

`Vec::from(array)` and `HashSet::from(array)` are ordinary associated library
functions. Do not recognize their names specially in HIR or executable lowering.
Their parameter is builtin `[T]`; return types identify the concrete collection.
Vec preserves order and duplicates. HashSet requires the checked `T: Eq + Hash`
contract, deduplicates through it and retains unspecified iteration order. An
existing generic conversion trait may supply the associated call only if its
current checked model expresses this exact contract; do not expand generic
conversion semantics or retain two competing constructor authorities.

Both constructors allocate independent collection storage and leave the input
array usable. Copy value elements according to their semantics; preserve shared
element identities. Never steal a named array's buffer or deep-copy its elements.
All literal element expressions complete before constructor execution, including
duplicate elements later eliminated by HashSet. Preserve roots and cancellation
during copying, hashing and any checked protocol calls. Failed construction must
not publish a partially valid result; prior observable effects remain completed.

Vec still implements List/MutableList, retains growth operations and follows
existing interface upcasts. Empty constructors infer elements from context and
generic arguments; unconstrained empty construction must not silently pick a type.
The `Vec::from([])` case should use the expected Vec element type where available.

Audit existing collection producers by meaning: List algorithms and FromIterator
that promise lists continue producing Vec or their declared List result. APIs
explicitly promising arrays, such as `to_array`, produce independent builtin
Array objects. String split and other existing Vec-producing functions must not
silently acquire fixed-length return contracts because they currently mention
`Ty::Array`. Preserve list algorithms, custom implementations, native defaults,
callback behavior and declared result types after their representation migration.

### Representation, host boundaries and validation

The semantic owner of fixed length is builtin Array; the owner of resizable Vec
is its registered nominal declaration. Reuse `NativeStorageLayout::Sequence`
where suitable for Vec instead of adding another HIR special case named Vec.
Shared element-buffer routines may serve both types, but a sequence layout alone
must never grant resizing authority over an Array object.

Array allocation must retain element contracts and generation ownership without
depending on a Vec provider. Keep compact element storage, GC tracing, roots,
owner checks and lease cleanup already required by the current paths. This task
does not require inline/stack array representation or a new collector. Decide
physical payload/tag details in BA01 and record the result here before editing
their consumers; no second independently implemented element semantics is allowed.

Resizing APIs, primitive entry contracts, native editing, reflection and host
conversion must reject a builtin Array where a growable Vec is required. This
must hold for source-free inputs as well as well-typed source. Conversely, an
Array operation must not accept an arbitrary nominal sequence with a matching
element layout. Keep independent declared-type, object-kind, owner/generation,
element, bounds and borrow validation at the responsible boundaries.

Rust `Vec<T>` conversions and ScriptVec represent the library Vec, including
retained identity and reentry-safe snapshots. Migrate their registration and
typed checks coherently. Audit `HostValueType::Array` and existing host collection
APIs by their prior contract: growable host Vec paths need an explicit matching
representation; builtin array paths expose no resizing. Use the existing host
registration model for these necessary changes; SA21 remains separate. Add an
array adapter only for a concrete embedding or constructor requirement, rather
than exposing unvalidated references or a speculative second public API family.

Portable types, hashes, codecs, native requirements and installation checks must
agree on the new identities. Preserve old pinned runtime generations while active;
do not reinterpret their live objects as a different family. Development artifact
caches may be discarded; no old-format reader, forwarding alias or routine
format/ABI version bump is required for this unpublished migration. Rebuild the
affected fixtures at a coherent checkpoint, retaining corruption/rejection cases.

### Later type-level lengths

`[T; N]` remains unsupported type syntax in this task. Future exact-length types
could refine `[T]` with a checked length, but their conversions, inference and
constant-expression rules require a separate decision. General const generics
are a larger feature. Keep object length distinct from semantic element identity
now; do not reserve unused fields, duplicate type variants or promise a future
ABI. Runtime length does not prevent compact storage or later optimization.

## Execution phases and build policy

| Phase | Work and exit evidence |
| --- | --- |
| BA01: establish representation and migration inventory | Trace the owners above, record the fixed/growable storage distinction and core-protocol installation path. Classify every existing Array producer, host adapter and array-interface role as intrinsic Array or library collection; choose concrete replacements for affected host schemas. Record affected test owners and source-free rejection paths. This is a bounded inventory, not a new abstraction project. |
| BA02: split types and checked operations | Separate builtin Array from nominal Vec in shared/HIR types, registration, mapping, substitution, rendering, literal/type resolution, native requirements and lowering. Update MIR/bytecode contracts and verifiers together. Remove Array-to-List shorthand/coercion and obsolete Vec intrinsic mapping after consumers migrate. Carry only valid checked operations to backends. |
| BA03: enforce runtime storage contracts | Implement fixed-length allocation/read/write/iteration and nominal Vec growth through the chosen ownership model. Migrate VM helpers, physical operation admission, host conversions, reflection and typed paths. Reuse safe buffer kernels and remove superseded special cases. Establish that low-level/source-free calls cannot resize or relabel Array. |
| BA04: integrate library construction and callers | Register Vec/HashSet array constructors; migrate algorithm results, FromIterator, generated declarations and actual application callers. Replace old growable literals with explicit constructors and old list annotations with List/MutableList where intended. Keep literal-based fixed-array use where appropriate. Exercise custom list implementations and default bodies after the split. |
| BA05: complete acceptance and documentation | Update specifications, architecture, HIR examples, public API docs, examples/benchmarks and fixtures coherently. Run focused boundary acceptance, remove obsolete migration paths and complete final local checks. Report the full CI feature/backend matrix separately. |

BA02-BA04 are one coupled integration checkpoint. Intermediate edits may fail to
build while dependent owners migrate; attempt relevant checks at architecture
boundaries and record each command, representative error, cause and owning phase
below. Do not repeat an unchanged known failure or claim a phase is integrated
merely because its files are edited. Run focused tests as soon as their units build.
All carried failures must be resolved before the integration checkpoint is committed
or BA05 is accepted. BA01 can be a separate documentation checkpoint.

Use one implementation of each final contract; do not keep a compatibility route
that still lets literals create Vec. No production stubs, bypassed validation or
weakened assertions may stand in for unfinished dependent work. Run structural
review/checking and diff checks at implementation checkpoints under
[repository policy](../AGENTS.md). Split growing responsibilities at their owners;
record justified structural debt and narrow exceptions in the existing ledgers.

## Acceptance and verification

Reuse existing fixtures, with new cases only for missing contracts. These are
acceptance responsibilities, not instructions to run every suite after each edit.

| Contract | Required evidence / existing owner |
| --- | --- |
| Type separation | HIR checks accept different lengths of `[T]`, runtime repeat counts, generic `[T]` parameters and contextual empty arrays. Reject implicit Vec/List interchange and array resize methods. Literal type checking must work without installed Vec/List. Existing HIR type and native-view tests own these cases. |
| Evaluation and aliases | [Array operations](../crates/kagari-embed/tests/array_operations.rs) retain value/count ordering, zero-count effects, repetition restrictions, nested value semantics and bounds errors; add fixed-length alias/rebinding and constructor isolation only where uncovered. |
| Collection behavior | [Collection interfaces](../crates/kagari-embed/tests/collection_interfaces.rs), [access](../crates/kagari-embed/tests/collection_access.rs) and [list algorithms](../crates/kagari-embed/tests/list_algorithms.rs) preserve Vec growth, readonly views, custom implementations, checked default dispatch and callbacks using explicit construction. |
| Constructor semantics | Extend collection/standard-declaration fixtures for Vec order/duplicates, HashSet Eq/Hash deduplication, independent storage with shared elements, empty inference and once-only element effects. Verify the calls are ordinary registered entries. |
| Executable integrity | Contract/MIR/bytecode verifier fixtures reject forged array-versus-Vec types, mismatched construction/resize operations and incorrect native binding/layout requirements; valid artifacts round-trip and execute without source-side reanalysis. |
| Runtime/host safety | GC/native/host fixtures cover foreign/stale handles, nominal Vec conversion, fixed-array resize rejection, live aliases, roots, reentry/lease conflicts and cleanup on trap/cancellation. Validate growth remains possible for legitimate Vec after migration. |
| Tooling and reload | Generated declarations, hover/display and navigation distinguish `[T]`, Vec and List. Cache facts follow changed installed declarations; retained snapshots and generation-pinned calls keep their original valid identities. Reuse registration-source and reload fixtures. |

Initial focused integration commands (select narrower tests during earlier work):

```text
cargo test -p kagari-embed --test array_operations
cargo test -p kagari-embed --test collection_interfaces
cargo test -p kagari-embed --test collection_access
cargo test -p kagari-embed --test standard_declarations
```

BA01 must select existing focused HIR, verifier, native/GC, list-algorithm and
registration selectors after confirming their current names. Retain distinct
source-free feature coverage; a round-trip test built with source support is not
proof of a no-source build. Native preparation may choose interpreter fallback:
record that honestly and do not claim native execution from JIT enablement alone.

At final acceptance of the entire migration, run the repository's local structure,
format, strict Clippy, workspace-test and diff checks once as a final batch. Fix
failures with focused tests; repeat a full run only to establish acceptance after
repairs. GitHub CI owns the complete feature/backend matrix. The initial planning-only
documentation checkpoint did not require a full build/test run.

Update [collection semantics](spec/collection-access.md),
[value semantics](spec/value-semantics.md), [syntax](spec/syntax.md),
[builtins](spec/builtins.md), [native declarations](spec/standard-declarations.md)
and affected host/architecture docs during BA05. Specifications now describe the
implemented separation. Benchmark programs
must preserve their workloads when construction spelling changes; measure any
claimed performance benefit independently.

## Progress and carried failures

- [x] Planning: choose runtime-stored fixed length, define scope and acceptance.
- [x] BA01: Record concrete representation and consumer inventory.
- [x] BA02: Split types and checked operations.
- [x] BA03: Enforce runtime and host storage contracts.
- [x] BA04: Integrate constructors and migrate callers.
- [x] BA05: Complete documentation and final acceptance.

### BA01 representation decision and inventory

The starting checkout was clean at `e8f64038`. Inspection confirms that both
semantic Array and its GC helpers currently grant Vec growth authority. The split
must therefore reach executable admission and host conversion, not only methods.

- Builtin `Ty::Array(element)` / HIR Array carry only the element. Keep
  `Value::Array` and the runtime-owned native payload descriptor (no installed Vec
  factory dependency). The payload retains `StorageType`, its owning generation
  and `SequenceStorage`; tracing and compact scalar storage remain shared.
- Vec is `Ty::NativeObject` with its actual registered declaration and
  `NativeStorageLayout::Sequence { element: 0 }`, represented by `Value::GcHandle`.
  Remove `NativeTypeConstructor::Array` and `NativeTypeKind::Vec`. Layout permits
  buffer access only after nominal declaration, object, element and generation
  validation; it never relabels an Array. Growth and `SequenceEdit` admission must
  require nominal sequence storage. Fixed-array replacement retains bounds and
  callback/iteration guards. Read kernels may be shared after family admission.
- `MakeArray`/`RepeatArray` construct only intrinsic arrays. Index place lowering
  carries builtin array facts independently of MutableList; Vec/List indexing
  uses checked installed Index/MutableList implementations. Preserve actual
  integer index checks and once-only place evaluation. Array core implementations
  (identity protocols, Index and Iterable), len/is_empty and their bindings belong
  to explicitly installed core registrations, not Vec or List declarations.
  Empty installations still type-check literal/type syntax without these methods.
- Array-interface registration roles remain solely for library List indexing and
  writable List places. Remove their syntax/literal contextualization authority;
  do not activate the broader SA18 access-marker removal.

Consumer classification (each row names the existing semantic owner):

| Consumers | Final contract / migration |
| --- | --- |
| HIR type syntax, body literal/repeat inference; compiler aggregate lowering | Intrinsic Array, with contextual element inference and repeat-value restrictions. |
| Type mapping, substitution, identity/hash/wire, physical representations, closed-type and trait proof traversal | Preserve Array recursively; nominal Vec follows existing NativeObject traversal. Update independent MIR/bytecode and primitive checks together. |
| `catalog/defaults`, `construction_defaults`, `declarations::vec`, namespace ownership and documentation examples | Nominal Vec; retain List/MutableList implementations and FromIterator. Add ordinary associated `from([T])` entries for Vec/HashSet. |
| `catalog/list_methods` mapped results; `catalog/strings` split results | Nominal Vec or the existing declared List result. Preserve order, callbacks and independent result storage. |
| `bindings/lists`, `lists/receivers` snapshots; construction aggregate fast path | List algorithms use nominal Vec work buffers and selected interface callbacks. Preserve the existing checked scalar reduction path for admitted compact storage. No registered `to_array` entry currently exists; any array-returning path must allocate fixed storage, never expose a growable buffer. |
| Rust `Vec<T>`, ScriptVec, standard `vec` authoring helper | Resolve the registered nominal Vec through declaration catalogs; retain roots, reentry snapshots and element contracts. They must reject builtin arrays. |
| `HostValueType::Array`, host matching, metadata, typed indexed paths | Array becomes element-only. Existing growable schemas need explicit nominal Vec declaration/element identities; readonly list exposure needs the declared List interface rather than readonly intrinsic Array. Retain declared path writeability and runtime borrow checks independently. Do not infer the family from Rust Vec payloads. |
| Native `Codec::Sequence`, mutable sequence codecs, primitives, context argument helpers, cursor sources, GC edit/lease/capacity helpers | Separate array admission from nominal sequence admission; all resizing and detached edits require the latter. Shared payload layout alone is insufficient. |
| VM dispatch, reflection, runtime value checks, GC kinds, native requirements and installation | Check the actual Array versus NativeObject family, owner and element contract; preserve generation-pinned identities and dependency validation. |
| Examples, benchmarks, HIR/native fixtures, artifact producers and source queries | Keep literals for actual fixed arrays; use explicit Vec construction and List annotations for library operations. Regenerate affected development fixtures once after integration; no ABI/version bump or legacy reader. |

Focused selectors confirmed in the checkout:

- HIR: `cargo test -p kagari-hir typeck`,
  `cargo test -p kagari-hir type_application_tests`,
  `cargo test -p kagari-hir --test language_contracts`.
- Executable contracts: focused array/native tests in `kagari-contract`,
  `kagari-mir` verification and `kagari-bytecode` verification/access. Forged
  native primitive signatures and layout requirements must reject Array/Vec
  interchange independently of source checking.
- Runtime: `cargo test -p kagari-runtime --test native_conversion`,
  `--test gc_ownership`, `--test typed_path_views`, plus GC array/sequence-edit
  unit selectors. Retain foreign handles, leases, cancellation and reentry cases.
- SDK: the four initial integration tests above, `--test list_algorithms`,
  `--test registration_sources`, `--test iteration_traits` and reload owners.
- Source-free: `cargo test -p kagari-embed --no-default-features --test artifact_features`;
  its existing forged native signature and algorithm fixtures are separate from
  source-enabled round trips. Regenerate their producer artifacts at BA04/BA05.

BA01 is a documentation checkpoint: content/diff checks only; no build or test
acceptance is claimed. BA02-BA04 remain one coupled checkpoint. No carried
implementation diagnostics exist yet. Record subsequent failed commands, causes
and owning phases here; final local acceptance and CI remain outstanding.

### BA02-BA05 integration history

The following entries retain the observations and outstanding work at each
intermediate checkpoint. Later repairs supersede earlier failure and pending-work
statements; the final acceptance record below owns the current outcome.

The working tree now separates element-only Array syntax/semantic types from
nominal Vec registration. Runtime array/sequence admission shares compact buffer
kernels; nominal sequence codecs, Rust Vec/ScriptVec conversion, checked indexed
assignment, iteration and ordinary Vec/HashSet array constructors are being
integrated. Host schemas distinguish fixed Array, explicit nominal Vec and List
interfaces. This work remains uncommitted until all carried integration failures
are resolved; the BA02-BA04 checkboxes are deliberately still open.

Observed local checks and repairs:

- `cargo check -p kagari-types` initially reported removed Array access bindings
  in access/wire traversal (BA02); repaired. Subsequent HIR/stdlib/compiler library
  checks passed at their respective boundaries, with unused-import warnings still
  awaiting cleanup. These checks do not establish downstream test acceptance.
- `cargo check -p kagari-stdlib` exposed the old ScriptVec/primitive access and
  buffer helper callers (BA03); migrated those owners to nominal sequence
  admission. Host metadata used a nonexistent Native kind during schema edits;
  corrected to the existing Struct metadata classification.
- `cargo test -p kagari-embed --test array_operations` initially exposed the new
  core array module's documentation inventory, old Vec annotations on repeat
  literals, and a constructor parameter accidentally named `self`. The latter
  caused both HIR receiver replacement and MIR `InvalidPublicAbi`; naming the
  ordinary associated parameter `array` fixed the actual declaration contract.
- The same array-operations command then passed all 6 tests, including the new
  fixed-length alias/rebinding and explicit-constructor isolation case. Existing
  artifact round trips and JIT-enabled preparation are exercised; actual native
  entry was not established, so this is not a native execution claim.

Outstanding BA02-BA04 work: migrate remaining library callers and host fixtures;
complete source-free family/layout rejection coverage, schema validation and
conversion checks; review compact-buffer access and all mutation/lease entrypoints;
remove obsolete array-access tests/models and temporary diagnostic tooling; run
focused collection, verifier, runtime, registration and source-free checks. BA05
specification/example/fixture migration, structural review, final local acceptance
and full CI are still outstanding. Temporary logs are under `target/sa20/`.


Further integration evidence (BA02-BA04, still not accepted):

- Collection-access and collection-interface SDK tests passed (5 each). The
  nested `List<Vec<i32>>` case initially failed MIR application validation in
  `core::iter`: concrete native arguments introduced a declaration owner absent
  from the module dependency closure. Source program assembly now visits the
  existing complete definition-reference inventory and pins referenced source
  owners, including generic arguments; host-only identities remain host contracts.
  This replaces the incomplete callable-only collection without weakening checks.
- Runtime unit tests passed 155 cases after migrating growth/lease fixtures to
  nominal sequences and preserving the existing ScriptTrap category for invalid
  fixed-array handles. Runtime native conversion passed 10 cases, typed host paths
  passed 20, and GC ownership passed its existing suite. Host schema/type unit
  tests passed 38; language-contract tests passed 9, including builtin syntax
  without installed declarations. New low-level Array admission checks are pending.
- `CallContext` now explicitly distinguishes `allocate_array` and `allocate_vec`;
  storage factories expose fixed `allocate_array`. The generic scoped sequence
  allocator belongs to native sequence storage, with standard Vec identity lookup
  retained in the Rust Vec adapter. Cursor item types come from the admitted
  sequence element contract, including nonzero storage element positions.
- The first list-algorithm pass had 7/8 successes; its cancellation test still
  interpreted Vec as Value::Array. The fixture is migrated without changing the
  cancellation, partial effects or root-cleanup assertions. VM list and native
  boundary fixtures still contain old literal, host-schema and representation
  expectations; their migration and reruns remain BA04 work.
- Import cleanup briefly left orphan cfg(test) attributes on three production
  items; those were removed. HIR unit fixture imports that had relied on parent
  test globs now import surviving CollectionAccess explicitly. Subsequent targeted
  checks, rather than the failed intermediate builds, determine acceptance.
- Structural scanning found seven new qualified-path issues, now addressed;
  a fresh structural check and manual ownership/import review remain required.


Final integration and BA05 acceptance in progress:

- VM library collections passed 18 cases and native boundaries passed 160. Host
  family cases additionally verify valid Array, Vec and List input/results and
  reject cross-family substitution before callbacks and at return validation.
- Compiler interface-normalization, ABI and verifier selectors passed. String
  interpolation exposed a real owner mismatch: its compiler-generated `[String]`
  temporary requires the intrinsic array contract, not a nominal Vec codec.
  Restored that contract with element validation; all 6 interpolation tests passed.
- The fixed-array/nominal-sequence facade, common buffer kernels and growable
  operations now have distinct GC modules. GC-focused tests passed 24 cases,
  including forged GcHandle labels, growth/capacity/edit rejection and unchanged
  contents/resources. Native conversion passed 10 cases, including empty and
  nonempty fixed-array rejection by Rust Vec and ScriptVec.
- Existing SDK collection interfaces, iteration, algorithms and registration-source
  suites passed. All 9 syntax-example tests passed source and artifact execution.
  Examples now use explicit Vec construction for list algorithms and usize indices
  for Vec's declared Index implementation; intrinsic Array retains integer indexing.
  Both expected Vec context and later-use element inference for Vec::from([])
  are covered by array operations and the existing inference suite.
- Indexed assignment now captures nominal receivers before index evaluation, as
  required for shared objects. The existing alias test verifies rebinding a Vec
  during index evaluation mutates the original object once, not the new binding.
- Source-free artifact_features passed all 10 cases with regenerated local fixtures,
  preserving reload loops and testing both Vec/HashSet constructors plus forged
  Array/Vec native signature rejection. Format/ABI identifiers were not bumped.
- Removed the temporary AST rewrite example. Structural scanning passed 1030 Rust
  files with zero violations/exceptions; manual review found no added production
  glob/re-export/include paths. Full workspace final acceptance is now running.
  Its first Clippy attempts found obsolete/missing test imports and an unnecessary
  lifetime; fixes are in progress. No final workspace or CI pass is claimed yet.

- First full-workspace execution (`cargo test --workspace --no-fail-fast`) found
  remaining fixtures that still paired Vec annotations with array literals or
  intrinsic host allocations, plus old List-index binding expectations. The
  affected compiler, enum, callable, reflection, numeric, never and transfer
  fixtures are being corrected at their existing semantic owners. Async verifier
  corruption now changes the element type (the removed access marker no longer
  distinguishes types); the rejection assertion is preserved.
- This acceptance pass also exposed two actual migration gaps: a terminating
  assignment receiver must bypass writable-interface selection after its subexpressions
  are checked, and nominal sequence setter bounds must retain IndexOutOfBounds.
  Both are repaired. Existing conformance growth/iteration fixtures now explicitly
  allocate/schema-check nominal Vec; fixed-array contracts remain in their own
  allocation/alias/constructor suites. All original conformance observations and
  cleanup assertions remain. Its complete route suite is still running.
- Clippy now passes across all workspace targets. Source examples for host reentry
  and collection iteration execute successfully. Final workspace tests are still
  in progress; later failures and final outcomes belong to this same ledger.

- Empty array arguments originally retained an Unknown contextual element, so
  Vec::from([]) could lose later-use constraints. Array inference now creates a
  solver variable when its expected element is Unknown; all 8 SDK inference cases
  and 21 HIR context cases pass, including conflicting later writes. Nominal type
  arguments also now participate in recursive standard constraint validation,
  retaining rejection of Vec<HashMap<f32, i32>> without weakening assertions.
- The complete observable language-contract suite passed across its source,
  artifact and JIT/fallback routes. The first full workspace pass completed with
  20 failing targets; all are tracked fixture migrations or the repaired semantic
  gaps above. A concurrent dependency rebuild also invalidated a rustdoc input;
  documentation tests will be rerun after builds settle. Final acceptance will run
  without concurrent source changes or alternate feature builds.

- Remaining VM owned-drive/GC fixtures now distinguish fixed arrays from retained
  nominal Vec values. The source-free native-wait fixture installs a minimal
  canonical Vec provider and records it in artifact dependencies; a typed native
  read retains the post-await element observation without applying an intrinsic
  Array opcode to Vec. Cold inputs and the live/dead values are explicitly rooted
  until execution owns them. All four native-wait cases pass, retaining snapshot,
  iteration-lease, cancellation and drop-cleanup assertions.
- Final focused repairs passed: HIR context (21), operator (9) and nominal (7)
  selectors; SDK type inference (8); runtime execution sessions (11) and host
  objects (10); VM growing-window GC; and documentation tests. The complete
  observable language-contract suite also passed again inside the final workspace
  run. Temporary logs and the full acceptance output remain under `target/sa20/`.
- Final manual review removed one redundant qualified production call and updated
  HIR syntax comments. These are import/comment-only corrections. Structure and
  format checks pass again; no production globs, re-exports, handwritten include
  paths or structural exceptions were added. Documentation file links pass.

- The second full run (`cargo test --workspace`, `workspace-acceptance.log`)
  reached the final native-boundary suite with one failure: its direct VecSet
  primitive test still expected ModuleValidation and the old array-set message.
  Nominal indexing now uses this same checked setter, whose bounds failure must
  be IndexOutOfBounds to preserve script indexed-assignment behavior. The fixture
  now asserts that exact kind, retaining its alias, committed prior write,
  unchanged failed element, readonly-input and cleanup checks. This is an error
  classification correction at the common setter, not a relaxed assertion.
  Owning phases: BA03 admission and BA04 caller assertions. A focused rerun and
  renewed final workspace acceptance follow; no successful full pass is claimed
  from the failed run.


### BA05 final local acceptance

BA01-BA05 are complete locally. There are no carried build/test failures or new
structural exceptions. The nominal primitive bounds test passed after its exact
error-category correction; the subsequent complete workspace run passed with
1,951 tests, zero failures and two existing ignored tests (114 result groups,
including documentation tests). All implementation changes are one coupled
BA02-BA05 checkpoint following the separate BA01 inventory commit.

| Check | Final result |
| --- | --- |
| `uv run --locked scripts/check_structure.py` | Passed: 1,030 Rust files, zero violations and zero documented exceptions. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed after all Rust edits. |
| `cargo test --workspace` | Passed; final output in `target/sa20/workspace-accepted.log`. |
| `cargo test -p kagari-embed --no-default-features --test artifact_features` | Passed: 10 source-free artifact cases; output in `target/sa20/source-free2.log`. |
| Documentation file-link review and `git diff --check` | Passed. |

Focused boundary checks and the host-reentry/collection-iteration examples also
passed as recorded above. Specifications, architecture, HIR comments, examples,
benchmark construction spelling, review status and roadmap now describe the
implemented Array/Vec distinction. Development fixtures were regenerated without
format/ABI bumps. No obsolete Vec-to-intrinsic-Array mapping, array growth API,
compatibility reader or temporary rewrite example remains.

GitHub CI's complete feature/backend matrix has not been run by this task and
remains open. JIT-enabled preparation may fall back to the interpreter; these
results do not establish universal native execution or a performance improvement.
SA18/SA19/SA21 and type-level lengths remain outside this completed migration.
