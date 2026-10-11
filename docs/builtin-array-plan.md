# Builtin fixed-length arrays (SA20)

Status: execution plan prepared; implementation has not started. The user selected
SA20 as the next implementation direction and requested this plan. The
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

Target examples, not current behavior:

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

## Current implementation and owners

Inspection found that the existing name Array describes growable Vec storage at
several boundaries. Removing methods from the source surface alone cannot enforce
fixed length. The migration must separate these identities and their operations.

| Owner | Current coupling and required work |
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
repairs. GitHub CI owns the complete feature/backend matrix. No full build/test
run is required for this planning-only documentation change.

Update [collection semantics](spec/collection-access.md),
[value semantics](spec/value-semantics.md), [syntax](spec/syntax.md),
[builtins](spec/builtins.md), [native declarations](spec/standard-declarations.md)
and affected host/architecture docs during BA05. Current specs remain the record
of implemented behavior until the migration is integrated. Benchmark programs
must preserve their workloads when construction spelling changes; measure any
claimed performance benefit independently.

## Progress and carried failures

- [x] Planning: choose runtime-stored fixed length, define scope and acceptance.
- [ ] BA01: Record concrete representation and consumer inventory.
- [ ] BA02: Split types and checked operations.
- [ ] BA03: Enforce runtime and host storage contracts.
- [ ] BA04: Integrate constructors and migrate callers.
- [ ] BA05: Complete documentation and final acceptance.

Implementation has not started. No implementation build or test results are
claimed, and no carried implementation failures have been observed. Record future
commands, diagnostics, responsible phases and local/CI outcomes here rather than
creating another progress queue.
