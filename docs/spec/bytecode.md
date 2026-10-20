# Kagari Bytecode

This document specifies the Kagari bytecode execution model.
The on-disk artifact encoding is versioned separately from the semantic bytecode model described here.

Execution-model context is defined in [execution.md](execution.md).
Backend abstraction rules are defined in [codegen-backend.md](codegen-backend.md).
Typed path mutation rules are defined in [typed-path-mutation.md](typed-path-mutation.md).

## Scope

This document defines:

- the semantic execution shape of bytecode
- the in-memory Rust model used by the compiler and VM layers
- the lowering boundary from IR into bytecode

This document does not define:

- the final on-disk `.kbc` binary encoding
- versioning and compatibility rules for serialized artifacts
- bytecode verification rules in full detail

## Design Goals

- keep bytecode as the primary interpreter target
- preserve a clean lowering path from non-SSA IR
- avoid coupling bytecode directly to AST or HIR structure
- keep the format compatible with SSA and machine-code backend work
- make VM execution straightforward and predictable
- represent host-backed typed path access explicitly
- carry enough metadata for hot reload validation and JIT lowering

## Position in the Pipeline

The lowering pipeline is:

1. source
2. syntax
3. HIR
4. verified concrete MIR
5. bytecode
6. VM execution

Optional backend paths include:

- verified MIR -> bounded MIR passes -> reverified MIR -> backend
- verified MIR + ABI/link descriptions -> JIT backend

Bytecode remains a first-class execution format when those backend paths are present.

## Verified MIR Boundary

Checked analysis can contain valid source imports. The snapshot-owned CheckedProgram
checks every reachable module, and `lower_program_to_mir` produces a VerifiedMirProgram
with preserved module boundaries and declaration-to-module/function bindings.
IR verification checks every imported signature against its bound target and rejects
missing dependencies, unrelated modules, unresolved calls, signature
mismatches and conflicting layouts for the same nominal struct declaration.
Editing any module invalidates the program's verification and link bindings.

`lower_program_to_bytecode` borrows VerifiedMirProgram and accepts caller cancellation.
It returns an immutable `VerifiedBytecodeProgram` retaining the verified graph:
one root ModuleRef and a stable vector of BytecodeModule members. Each
member stores its dependency ModuleRefs. Module identities are unique; all members
must be reachable from the root. Calls to dependencies use
`ModuleFunction { module: ModuleRef, function: FunctionRef }`, scoped to that program.
Verification checks dependency reachability, target signatures, shared nominal layouts
and consistent host contracts. Unused imports remain linked but execute no code.
The unit-only `lower_to_bytecode` rejects source dependencies with UnlinkedSourceModules;
it cannot discard dependencies to form a standalone artifact.

`lower_to_mir(checked, options)` returns an immutable `VerifiedMirModule`.
`lower_to_bytecode` accepts only that handle. An optimizer or inspection tool can
consume it with `into_unverified()`, modify the resulting `MirModule`, and call
`verify_mir(module, cancellation_token)` to obtain a new verified handle. There is
no unchecked bytecode-lowering entry point.

Verification checks instance identity and uniqueness, direct-call targets and
signatures, parameter/local layout, operand annotations, operation representations,
return and branch types, block targets and terminators, debug-table alignment,
read-only paths, and conservative instruction effects. Standard calls and numeric
operations share representation contracts with the bytecode verifier; intrinsic
arity comes from the standard declaration table. IR rejects unsupported indirect
calls. The declared entry block is emitted first, even when its arena index is
nonzero; branches use offsets computed in that same emission order.

Every bytecode function ends in an explicit `Return`, `Jump`, `Branch`,
`PredicateBranch`, or `Unreachable`. An empty function or a final ordinary operation
cannot implicitly return `Unit`; the verifier rejects fallthrough before artifact construction or
runtime loading. If a verified execution ever reaches the end without a terminator,
the runtime treats that as an engine invariant failure.

IR struct construction names a nominal declaration and provides each field slot
exactly once. Ordinary field operands contain a declaring struct identity and a
slot, with no field-name fallback. The module carries checked layouts with field
identities, concrete ABI types and writeability. Operand representations are derived
from those types. Verification rejects duplicate or
malformed layouts, absent owners, invalid slots, missing/duplicate initializer
slots, mismatched operand representations and writes to read-only fields.
Initializer expressions retain source evaluation order; bytecode emission arranges
their already-computed values in declaration order. Run the `layouts` example in
`kagari-compiler` to inspect this boundary without runtime registration.

Bytecode carries the same verified `StructLayout` records. `StructId` indexes
this version's layout table; a field operand is `FieldRef { structure, slot }`.
`MakeStruct` carries a layout ID and positional slot operands, with exactly
one value per layout slot. The old field table, field-name operands and named
initializer records are removed. Artifact verification independently checks the
layout identities, field count, slots, representations and write permissions.
Executable aggregate identities use one top-level declaration segment with occurrence
zero. Field and variant identities append one member segment, also with occurrence
zero. Extra parent segments and recovery-only duplicate identities are rejected by
both IR and bytecode layout verification. HIR retains duplicate identities for
diagnostic recovery, but they cannot reach execution.

Interface allocation uses an explicit `MakeInterface dst, value, module,
implementation` instruction. Typed IR names the implementation declaration and ordered type arguments;
bytecode carries a dependency-program module slot and an `InterfaceTableRef`
into that member's verified table. Program verification requires the target
member to be reachable from the caller's dependency graph.
Verification requires a concrete table instance, a heap destination and the
receiver representation declared by the table. The runtime checks the full
receiver ABI, allocates a generation-checked interface object and retains its
linked execution version. For example, a verified `i32` receiver can be boxed
through its `Tag for i32` table; a bool receiver or out-of-range table slot is
rejected before execution. Source-level conversion emits this instruction when
the checked semantic result selects a unique implementation template and its
arguments. Generic instances compile all interface methods ahead of execution
and reuse one table per declaration/argument key; a generic template record is
not itself an executable table.

Interface calls use a dedicated `CallTarget::InterfaceMethod` containing the
trait owner's module slot, applied interface identity and declared method
ordinal. IR and bytecode verification check the trait signature, physical
argument/result types and dependency reachability. The runtime checks the
object's exact interface identity and full nominal method ABI, then enters the
implementation's retained version on the same explicit frame stack.
Executable method slots carry `CallableTarget::Script(FunctionRef)` or
`CallableTarget::Native(NativeImportId)`, scoped to the implementation module.
Verification checks the exact implementation identity, concrete arguments, binding
and normalized method signature. Native targets enter a rooted callable frame and
use the same bounded continuation/callback driver as direct native imports.
They do not require a fabricated script body. The frame pins the implementation
generation until return and obeys call-depth limits and cooperative cancellation.

MIR records each concrete native interface target with its complete native import
contract. Bytecode lowering consumes these verified applications; it does not
rebuild a native method signature or choose its callback dependencies.

Native imports may carry ordered, statically selected trait-member dependencies.
The declaration owns each receiver/interface/member requirement; its concrete
application records target identity, type arguments, implementation kind and full
signature. Program verification reproduces the selection from checked portable
implementation facts and requires a matching executable target in the dependency
closure. Native code requests a slot with the complete argument list. The shared
driver checks arguments and results and pins the selected implementation version.

Public struct templates are checked against each executable instance after generic
substitution: field count/order, names, permissions and concrete types must agree.
Program verification also checks imported instances against the declaring module,
including templates with no local executable instance. A standalone module cannot
prove correspondence to an absent dependency; the complete program check is required.
The VM checks the actual receiver's retained layout before slot access. Reflection
resolves names explicitly from this metadata and shares the checked write path.

Locals and temporaries must be initialized on every reachable predecessor before
use. This is a fixed-point analysis for non-SSA IR, including loops and merge
temporaries written in separate branches. Structural and type checks include
unreachable blocks, while definite initialization concerns reachable execution.
Only parameter locals begin initialized. A loop backedge cannot initialize a
value for its first iteration.

Counts are checked before conversion to bytecode ID widths. Verification observes
analysis cancellation, including public-template indexing and each layout/member
comparison, and caps its block/value dataflow matrix at 64 MiB. Failures
carry `KG_IR_*` codes, function/block/instruction positions and available source
spans. During source compilation, encoding or verification-state limits become
`KG_COMPILE_LIMIT_EXCEEDED` diagnostics.

This boundary validates declared layouts but does not yet prove the exact nominal
type of every heap-valued operand. Runtime host calls validate nominal object types
against their linked declarations; ordinary script call operands still carry representation contracts.
Struct fields carry complete nested types. Allocation and replacement validate
nominal layouts and nested tuple/container values before committing; operand
verification still checks physical representations rather than proving nominal flow.
Untyped host container mutations do not maintain a permanent element-type invariant.
The artifact loader runs bytecode verification independently. The MIR seal is
neither serialized nor a substitute for artifact validation. MIR supplies logical
root/liveness facts; native physical maps remain a backend responsibility, and
unsupported GC-bearing native functions use interpreter fallback.

## Execution Model

Kagari bytecode uses one function-local logical `Slot` namespace for parameters,
source binding homes and temporary values. It is not a stack-machine format.
`Slot` wraps u32; `Slot::new(usize)` uses checked conversion and panics on overflow
rather than truncating. Compiler/verifier/codec limits bound counts before execution;
`BytecodeFunction::slot_count` is u32, not a sum of two u16 storage counts.

MIR retains separate semantic locals and temporaries. Compiler bytecode lowering
projects their full contracts into this unified namespace; it does not replace
MIR's model or make a physical representation sufficient evidence for aliasing.
Runtime preparation separately maps logical slots to scalar/managed bank locations.
Machine-code backends still consume verified MIR, not interpreter slot placement.

`metadata.parameter_slots[arg_index]` explicitly selects each parameter's home.
The homes must be distinct, in range and agree with `metadata.params` and retained
semantic parameter facts. They need not be the first N slots. Only admitted
parameter homes start initialized; ordinary operations read inputs before defining
their output, including when the destination is also an input.

The bounded CFG verifier checks synchronous and suspendable functions: reachable
predecessors meet initialization by intersection, writes invalidate old semantic/
constant/access facts, and joins conservatively retain shared facts. A backedge
cannot initialize a value for the first iteration. Structural/type checks also
cover unreachable code. Exhausting a verification budget rejects the input.

### Semantic projection and placement

`FunctionMetadata::semantic` is `BytecodeSemanticSlots`, with `generic`,
`protocol_adapter`, parameter-indexed `params`, `result` and logical-indexed `slots`.
It preserves full `Ty` facts, including nominal arguments, reference access and
narrow numeric widths, separately from `metadata.slots: Vec<ValueType>`.
Missing annotations do not become unchecked `Any`; generic representations require
scoped generic facts. Capture-cell slots retain a managed representation even when
their semantic type describes scalar contents. Readonly-weakening moves and
managed/cell/call/await snapshots keep their independent checks.

Initial compiler placement uses MIR local homes followed by temporaries. Bounded,
block-local selection forwards proven stable scalar reads and redirects an eligible
scalar producer's sole adjacent assignment into its binding home only when full
types and required debug states agree. For example, `x = x + y` may become one
checked operation with `dst=x`: it reads both inputs, computes, and commits only
on success. Overflow leaves the old `x` intact. Calls, awaits, managed/cell writes
and compound path writes are not redirected by this pass. Uncertain snapshots stay
explicit `Move` operations. Final PCs, branch targets, debug ranges and origins are
repacked after selection; unused temporary slots are removed.

Runtime physical reuse is a separate bounded preparation step. CFG liveness,
parameter entry and retained binding availability contribute conservative intervals.
Disjoint scalar lifetimes may share a physical location; managed slots currently
keep distinct locations. Debug bindings map to logical slots, not permanent mirror
storage. Exhausting allocation's budget declines reuse without weakening verification
or changing the final instruction boundaries. See
[public frame access](execution.md#public-frame-access).

## Module Layout

The module model is:

```text
BytecodeModule {
  identity: ModuleIdentity,
  source_name: String,
  functions: [BytecodeFunction],
  module_slots: [ModuleSlot],
  constants: ConstantPool,
  types: TypeTable,
  paths: PathTable,
  public_items: PublicItemTable
}
```

Bytecode modules carry both executable instruction streams and the metadata needed to validate execution against the runtime, host registry, and hot reload coordinator.

Each function contains:

```text
BytecodeFunction {
  id: FunctionRef,
  identity: ConcreteFunctionIdentity?,
  name: String,
  parameter_count: u16,
  slot_count: u32,
  instructions: [BytecodeInstruction],
  metadata: FunctionMetadata
}
```

Function metadata includes:

- source span table, if available
- parameter and return layout
- logical slot representations and full retained semantic projection
- explicit argument-to-slot parameter homes
- mandatory semantic-origin-to-final-PC mapping and actual-origin selections
- effect summary
- safepoint metadata
- public ABI fingerprint, when public

## Implementation Status

The unified slot, semantic projection and selected instruction contracts above are
implemented in compiler lowering, independent verification, canonical KBC encoding,
runtime preparation, debugger/disassembly and native/AOT correspondence. The module
and function sketches above omit declaration/layout/link details for brevity.
There is no separate bytecode `Register`/`LocalSlot` API or local transport opcode.

These are current implementation contracts, not a claim of integrated performance
or final acceptance. BO04 performance remains blocked; its status is owned by the
[active plan](../bytecode-optimization-plan.md). Format/ABI identifiers are unchanged
for this unpublished replacement; obsolete development artifacts/caches are disposable.

## Operand Model

Bytecode operands are compact index-like values.
The operand model includes:

- `Slot`
- `ScalarOperand::Slot(Slot)` or `ScalarOperand::Constant { constant: ConstantId, ty: BuiltinType }`
- `ModuleSlot`
- `JumpTarget`
- `ConstantId` into the module's canonical `ConstantOperand` pool
- `CallTarget`
- `FunctionRef`
- `NativeImportId`
- `RuntimeHelperId`
- `TypeId`
- `PathId`
- `FieldId`, if ordinary script fields are indexed directly
- `DynamicArgList`, for path operations with runtime index operands

Rust representation types are implementation-defined.
Operands must not require runtime string lookup in ordinary compiled execution paths.

String names are valid in debug metadata, diagnostics, or early implementation placeholders.
They are not the semantic identity of hot bytecode operations such as field access, host path mutation, function calls, or type checks.

## Instruction Families

The bytecode instruction set covers:

- `LoadConst`
- `Move`
- unary arithmetic and logical operations
- binary arithmetic and comparison operations, including `BinaryOperands`
- `PredicateBranch` for a comparison used only to select control flow
- `Call`
- `Jump`
- `Branch`
- `Return`
- `Unreachable`

The complete bytecode instruction model also includes:

- aggregate construction
- ordinary script aggregate access
- host-backed typed path access
- interface dispatch or helper calls, when needed
- runtime helper calls
- module slot load/store
- type checks and downcasts

## Core Instruction Shape

The core instruction set has this shape:

```text
LoadConst dst, const
LoadModule dst, slot
StoreModule slot, src
Move dst, src

Unary dst, op, operand
Binary dst, op, lhs_slot, rhs_slot
BinaryOperands dst, op, lhs_operand, rhs_operand
PredicateBranch op, lhs_operand, rhs_operand, negated, then_target, else_target

Call dst?, callee, args
Jump target
Branch cond, then_target, else_target
Return value?
Unreachable
```

Value operands and destinations use `Slot`; `BinaryOperands` and
`PredicateBranch` use the explicit slot-or-typed-constant form. Control flow remains
explicit. `BinaryOperands` computes a numeric result (including numeric comparisons)
into a slot. `PredicateBranch` publishes no Boolean value: it evaluates its original
comparison, applies `negated` to that Boolean, then selects the corresponding arm.
It supports equality, identity equality and ordered comparisons under the ordinary
comparison contracts, not a special loop instruction.

Polarity is not predicate inversion. For example, `!(a < b)` is not rewritten as
`a >= b`: with NaN, the former is true and the latter false. Operand order, exact
numeric widths, primitive protocol eligibility and statically known object identity
categories remain independently checked for the new forms.

Compiler selection combines only control-only, single-definition comparisons and
eligible `Not` chains with compatible debug states and no intervening retained
operation. A Boolean stored in a source binding or used as data remains a value.
Selection does not fuse instructions during runtime preparation.

## Aggregate Instructions

Bytecode distinguishes ordinary script-owned aggregate access from host-backed path access.

Ordinary aggregate instructions:

```text
MakeTuple dst, elements
MakeArray dst, elements
MakeClosure dst, function_ref, captures
MakeCell dst, initial_value
ReadCell dst, cell
WriteCell cell, value
MakeStruct dst, type_id, field_values
ReadField dst, base, field_id
WriteField base, field_id, value
ReadIndex dst, base, index
WriteIndex base, index, value
```

These instructions apply to script-owned values or runtime-managed aggregates.
They must not silently become reflection over host-owned Rust state.

`MakeClosure` binds ordered capture values to a verified function slot. Indirect
closure calls carry a physical parameter and result contract; the runtime checks
the stored function against that contract before entering its frame. Capture
cells preserve writes to lexically captured `var` bindings and are traced by the
same non-moving GC as other script objects. A closure retains its linked program
version, including the dependency set used by subsequent calls. Invalid slots,
capture counts and slot types are rejected during bytecode verification.

If a field or index chain is resolved as host-backed, it lowers to the typed path instruction family.

## Typed Path Instructions

Host-backed field and index chains are represented explicitly.

Typed path instructions include:

```text
ReadPath dst, root_or_view, path_id, dynamic_args
SetPath root_or_view, path_id, dynamic_args, value
ModifyPath dst?, root_or_view, path_id, dynamic_args, op, value
MakePathView dst, root_or_view, path_id, dynamic_args
```

Opcode names are implementation details.
The semantic properties are:

- `path_id` identifies a typed path descriptor, not a string path
- `dynamic_args` contain runtime index values such as `item_id`
- the path descriptor records root type, result type, access policy, and schema or ABI fingerprint
- path operations can call host code, trap, validate declared access, validate epochs, and mark dirty state

Example source:

```kagari
player.inventory.items[item_id].count -= 1
```

Bytecode shape:

```text
Move         s0, s_player
Move         s1, s_item_id
LoadConst    s2, 1
ModifyPath   _, s0, path_Player_inventory_items_count, [s1], SubAssign, s2
```

Example with a local path view:

```kagari
val combat = player.combat
combat.hp -= damage
```

Bytecode shape:

```text
Move          s0, s_player
MakePathView  s1, s0, path_Player_combat, []
Move          s_combat, s1

Move          s2, s_combat
Move          s3, s_damage
ModifyPath    _, s2, path_Combat_hp, [], SubAssign, s3
```

Here `s_player`, `s_item_id`, `s_combat` and `s_damage` label logical binding
homes, not a second operand namespace; numbered slots retain once-only snapshots.
The managed/path operations are not direct-destination scalar selection candidates.

`MakePathView` must create a lightweight root-plus-path handle.
It must not create or store a Rust `&mut` reference.

## Path Table

Bytecode modules that use typed path mutation contain or reference a path table.

The path table model is:

```text
PathTable {
  paths: [PathDescriptor]
}

PathDescriptor {
  id: PathId,
  root_type: TypeId,
  result_type: TypeId,
  segments: [PathSegment],
  access: ReadOnly | ReadWrite,
  dynamic_arg_count: u16,
  abi_fingerprint: AbiFingerprint
}
```

The path table is produced by the compiler, module loader, host binding generator, or a combination of those components.
The bytecode interpreter treats `PathId` as an already resolved path identity.
It does not resolve ordinary path operations by repeatedly looking up field-name strings.

## Calls and Helpers

Calls distinguish:

- direct script function calls
- indirect slot-based calls (`CallTarget::Slot` and the typed
  `CallTarget::ClosureRegister` variant, whose `register` field is a `Slot`)
- host function calls
- runtime helper calls
- standard intrinsic calls with verified argument and result representations
- verified interface method calls

The `CallTarget` model records this distinction.

Host calls and runtime helpers remain explicit because they can:

- trap
- allocate
- trigger declared interface checks
- become safepoints
- enter frame-scoped host borrow guards
- interact with hot reload metadata

Typed path operations are represented as dedicated instructions or as strongly typed runtime helper calls.
Dedicated instructions preserve path structure for interpreters and machine-code backends.

## Effect Metadata

Instructions and instruction families are classifiable by effect.

Effect flags include:

- `may_allocate`
- `may_trap`
- `may_call_host`
- `may_access_reflection`
- `may_mutate_script_heap`
- `may_mutate_host_state`
- `may_mark_dirty`
- `may_suspend`
- `is_safepoint`

Effect classification:

```text
Move            value transfer; retain semantic/access checks
Binary          may_trap for checked arithmetic or invalid operands
BinaryOperands  same checked numeric semantics as the corresponding binary operation
PredicateBranch comparison contract plus explicit control transfer
MakeArray       may_allocate, is_safepoint
Call host       may_call_host, may_trap, is_safepoint
ReadPath        may_call_host, may_trap
SetPath         may_call_host, may_trap, may_mutate_host_state, may_mark_dirty
ModifyPath      may_call_host, may_trap, may_mutate_host_state, may_mark_dirty
```

This metadata supports:

- bytecode verification
- interpreter resource accounting
- GC safepoint handling
- host interop safety
- JIT lowering
- optimization passes over IR or bytecode-like forms

## Verification Requirements

The bytecode verifier checks:

- logical slot indices/counts/layouts agree and are in range
- explicit parameter homes are distinct, in range and type-compatible
- reads are definitely initialized along every reachable predecessor
- semantic/access/constant facts follow writes and conservative CFG joins
- mandatory origin mappings cover final PCs in order, with explicit actual-origin
  disambiguation for many-to-one groups at the artifact boundary
- jump targets point to valid instruction boundaries
- functions return values compatible with their declared return type
- instruction operands match the expected type layout when type metadata is retained
- runtime reflection helpers have the same verified arity and representation
  contract in IR and bytecode: field/index receivers are heap values, indexes
  are integers, reflective writes cannot store host handles, and result
  slots match the helper's static result where known
- field/path writes and semantic reference access respect their checked writeability
  contracts; source `val` / `var` binding rules remain owned by source/MIR checking
- `PathId` operands exist in the path table
- path dynamic argument counts match the descriptor
- path result values are used with compatible types
- write operations target writable paths
- public function signatures are concrete and ABI-supported
- private trait contracts have local canonical identities and do not duplicate
  public trait ABI records; host trait tables match the defining public or
  private method contract before publication
- local interface implementation tables match the defining public or private
  trait method roster and substituted signatures before their method slots can
  be used as executable bound evidence
- concrete host trait arguments satisfy every applied trait bound through
  exactly one matching script interface table or host trait table in the
  executable dependency closure. Matching uses declaration identity, ordered
  type arguments, generic substitution and recursive implementation bounds.
  Artifact verification caps the implementation catalog, candidate checks and
  proof depth; exhausting any limit rejects the product before execution
- modules in one dependency closure agree on each host type identity, symbol
  and executable declaration before runtime binding

Verification rejects malformed bytecode before execution.
Runtime checks are still required for host object liveness, dynamic index validity, declared access, and host-side invariants.

## Hot Reload Metadata

Bytecode modules are reload-aware.

Module metadata includes:

- module identity
- module epoch
- dependency identities and epochs, if needed
- public function ABI fingerprints
- referenced type fingerprints
- referenced host function fingerprints
- referenced path ABI fingerprints

Reload validation happens before publishing a new module epoch.
Failed validation must not replace the currently active epoch.

Existing active calls continue with the bytecode and metadata of the epoch they entered.
New calls use the latest successfully published epoch.

## GC and Safepoint Metadata

`RootSlotLayout::slots` is the verified, function-wide conservative logical root
layout derived by `RootSlotLayout::from_types(metadata.slots)`. Every representation
whose `ValueType::may_contain_gc_reference()` is true appears exactly once in
ascending logical slot order; extra or missing entries reject verification.
This portable layout currently selects `HeapObject` and `Generic`; scalar, string
and host-handle representations do not appear in that logical list. Runtime frames
separately trace the whole managed bank, including strings/host descriptors and
suspended callers, with complete checked identities.
Physical scalar reuse does not rename the portable logical root contract.

Additional GC and safepoint metadata planned for later backends includes:

- which logical values contain GC-managed references at safepoints
- which logical values contain host handles or path views
- which values are ephemeral and must not cross suspension points
- call boundary metadata for host calls and runtime helpers

Machine-code backends require equivalent stack maps or root maps.
Bytecode must not hide value liveness or helper boundaries that those backends need for equivalent root maps.

## Debug Metadata

Bytecode supports source mapping.

Debug metadata includes:

- instruction-to-source spans
- line table entries
- safe debug point markers
- local variable names and live ranges
- parameter names and live ranges
- captured binding names and locations
- frame layout metadata
- function names and module names
- path names for diagnostics
- type names for diagnostics

`LocalLiveRange { slot, binding, name, span, start, end, ty, is_parameter }`
separates source binding identity from its logical execution home. Ranges are
half-open final-PC intervals; runtime preparation includes retained availability
when allocating physical storage. A name does not reserve a permanent physical slot.

Optional debugger records are not execution type authority and may be stripped.
Mandatory origins below are a separate executable contract, not optional debug data.

## Executable origins and final PCs

`FunctionMetadata::origins` is indexed by semantic origin ID: compiler output retains
the function-local MIR emission ordinal (instructions and terminators). Each
`InstructionOrigin { instruction, span, line, column }` maps that origin to a final
executable PC and retains its own diagnostic coordinates. The nonempty bounded
mapping is nondecreasing, covers every final instruction without gaps and contains
no out-of-range PC. Several origins may map to one selected instruction; an origin
ID must never be interpreted as a PC merely because its number is in range.

`instruction_origins: Vec<usize>` selects exactly one actual origin for each
many-to-one PC group, in PC order; a unique-origin PC needs no selection entry.
`FunctionMetadata::instruction_origin(pc)` resolves this actual origin, not the
first removed transport alias. Canonical encoding/decoding rejects missing or
invalid disambiguation independently of source and optional debugger metadata.
For example, if a removed outer read and a retained negation share a PC, overflow
must identify the negation's span, not whichever record came first.

Interpreter traces use the selected origin of the actual final PC. Native polling
and trap records carry MIR origin IDs and runtime maps them to final PCs while
retaining the exact native origin. Missing line/column information stays missing;
it is not borrowed from another origin sharing that PC. These facts survive
source-free loading and optional debug stripping. See [error reports](error-reporting.md#bounds-versions-and-source-positions)
and [AOT validation](aot.md#bundle-and-trust-contract).

## Disassembly output

[`kg disasm`](toolchain-tools.md#kbc-disassembly) renders the complete current
instruction model from a [fully validated artifact](artifacts.md#offline-disassembly-validation).
It is instruction inspection, not source reconstruction. Full output contains an
artifact summary, referenced descriptors and selected function bodies; directory
mode retains only the summary and compact headers.

### Instructions and scoped references

Every selected instruction is printed once in stored order, including unreachable
instructions. Its number is a **zero-based function-local instruction index**, not
a file byte offset or machine PC. Decimal indices have at least four digits and
expand as needed; jump destinations have `L<index>` labels and visible numeric
targets. Explicit mnemonics and operands distinguish instruction variants and nested
call/numeric/iteration operations, including unified slots, typed constant operands,
predicate polarity, branch arms, optional
results, ordered arguments, captures, generic arguments and member ordinals. There
is no execution, optimization, control-flow restructuring or inferred callee body.

References retain artifact-local owning-module/table slots beside resolved portable
names and types. For example, `@1:const#2` addresses module 1's constant slot 2,
not slot 2 in the function's current module. Process-local definition IDs are not
artifact offsets. Referenced constants, layouts, interfaces/trait contracts, paths,
native imports and module slots (including their descriptor dependencies) are
printed once per output operation in module/category/slot order. Unrelated tables
are not dumped; calls do not recursively expand callee bodies. Interface dispatch
shows its declared member/signature without inventing a concrete receiver target.
Native imports describe declared call contracts, not provider implementations.

Path descriptors print only retained executable `PathRecord` facts: owning slot,
path ID, contract fingerprint, root/result physical types, readonly access and
escaped debug name. The fingerprint is not a declaration-vector index. Rendering
does **not** rebuild host schemas or expand source path declarations from it.

Constants retain their stored type. Every `f32`/`f64` constant includes its exact
hexadecimal bits beside decimal text, preserving signed zero and NaN payloads.
Large strings are emitted at their pool entry rather than repeated at each use.
Artifact-owned names, strings, URIs and diagnostics are escaped so they cannot
inject instructions or terminal controls. Quoted text escapes quotes, backslashes,
newlines, tabs, other controls, Unicode format/bidi controls and line/paragraph
separators while retaining ordinary Unicode text. Name components requiring quoting
are escaped individually; displayed names are not the raw CLI selector syntax.

### Execution facts and debug annotations

Headers prefer retained semantic signatures and concrete instance arguments;
missing signature facts are explicitly labeled `physical(...)`. Full bodies list
every logical `sN` slot with separate physical representation and retained semantic
type, or `semantic=unknown`. Unknown does not mean `Any`, and `heap_object` does
not identify a nominal source type. Root slots and retained generic/protocol-adapter
contracts are shown separately.

Entry mappings are `argN -> sM`, read from `metadata.parameter_slots`, not inferred
from a slot prefix or debug parameter flags. A subsequent `move s3, s0` transfers
a value; it does not establish a permanent alias or move a binding name from `s0`
to `s3`. The listing shows logical slots, not runtime bank offsets or native registers.

`recorded_effects` lists set `FunctionMetadata.effects` fields in this order:
`reads_local`, `writes_local`, `reads_module`, `writes_module`, `reads_aggregate`,
`writes_aggregate`, `reads_path`, `writes_path`, `allocates`, `calls`,
`touches_runtime`, `may_trap`, `may_suspend`; the empty set is `[]`. These are
recorded summaries, **not recomputed transitive effects**, permissions, purity or
execution-cost guarantees. An unset bit does not prove callees cannot allocate or
mutate. The separately displayed `may_suspend` is an execution-entry fact, not
proof of a particular original source spelling of `async`.

Debug annotations are optional and nonauthoritative:

- Locations are associated by explicit `instruction_offset`, not metadata vector
  position. Unsorted records work; multiple matching locations/points are retained
  in stable order and marked ambiguous rather than silently overwritten.
- Binding names annotate their `LocalLiveRange::slot` (`sN`) only within half-open
  `[start, end)` final-PC ranges. Overlapping names remain visible and ambiguous.
  Generated `argN` and `sN` labels remain usable when names are absent. Capture names
  do not imply a slot mapping that the artifact does not retain.
- Debug types, parameter flags and copied frame layouts cannot override execution
  metadata. Verification does not promise sorted/unique records, nonoverlapping
  names or agreement between debug claims and executable facts; disagreement is
  not a new renderer-specific validation failure.
- URIs, line/column values, source byte offsets/spans and source-module references
  come only from retained metadata. Missing line information uses available byte
  offsets/spans as such, never guessed line numbers. A default empty span is
  unknown. No source path is opened and no server is contacted.
- Missing/stripped debug data does not prevent inspection. Per-function metadata
  is inspected independently of the optional envelope debug section.

Output is deterministic for the same artifact and options, independent of fresh
process-local IDs, hash-map iteration, current directory and runtime registration.
It is human-readable text, not a stable machine protocol or a source/text round
trip. See [inspection ownership and design references](../architecture.md#offline-bytecode-inspection).

## Artifact Tables

The `.kbc` artifact is organized around compact tables rather than repeated inline strings.

Logical tables:

- constants
- functions
- module slots
- types
- host functions
- runtime helpers
- typed paths
- public items
- debug metadata

The binary encoding is a versioned artifact detail.
The logical separation is reflected in the in-memory model so the artifact format can evolve without changing bytecode semantics.

## Current In-Memory Shape

The current code shape lives in:

- [bytecode/lib.rs](../../crates/kagari-bytecode/src/lib.rs)
- [bytecode/module.rs](../../crates/kagari-bytecode/src/module.rs)
- [bytecode/instruction.rs](../../crates/kagari-bytecode/src/instruction.rs)
- [compiler/bytecode](../../crates/kagari-compiler/src/bytecode.rs)

The current implementation defines the execution-layer boundary and wraps bytecode in versioned `.kbc` artifact metadata.
Artifact serialization is implemented separately from bytecode semantics so the binary exchange format can evolve without changing VM behavior.

The `.kbc` section directory is derived from the program, debug payload and
signature payload in one fixed order. Loader validation rebuilds that directory
and rejects mismatched section identifiers, record counts, fingerprints and
source/debug name tables even when the outer content hash is consistent.

The decoder rejects inputs above 64 MiB. Before verification or publication,
artifact construction, decoding, encoding and in-memory loading also reject more
than 1,024 modules, 65,536 functions, 1,000,000 instructions, or 1,000,000
records in a module table or metadata table. Section-declared counts have the
same bound independently of payload size. These are format resource limits;
compiler instance and diagnostic budgets are separate.
Directly nested layout fields, enum variants and payloads, host type members and
parameters, host path segments, and public ABI declaration members each have a
4,096-record vector limit; their combined count across the program is limited
to 1,000,000. ABI types use flat preorder nodes with limits of 4,096 nodes and
depth 64. Encoded input size, executable operands, identity paths and metadata
vectors have corresponding preflight limits in the artifact format.

## Relationship to IR

Bytecode is the flattened interpreter target of verified MIR.

IR is responsible for:

- explicit basic blocks
- control-flow structure
- language-to-execution lowering
- future SSA transition points

Bytecode is responsible for:

- linear instruction streams
- VM-friendly operands
- the direct execution contract for the interpreter

This means bytecode is not the only internal execution representation.
IR remains the better place for control-flow construction and future optimization work.

## Block Flattening

IR uses basic blocks.
Bytecode uses a linear instruction stream.

The lowering rule is:

- choose a block order
- emit each block's instructions into a flat instruction list
- map each `BlockId` to a `JumpTarget`
- rewrite IR terminators into bytecode jumps and branches

The `JumpTarget` representation used by the VM is final-instruction-stream based,
not block-ID or semantic-origin based. Selection repacks PCs after removing eligible
transport or selecting predicates/constants; it then rewrites all branch arms,
origin mappings and optional debug offsets/ranges.

## Constant Handling

`LoadConst` carries a `ConstantId`, indexing the owning module's portable
`ConstantOperand` table. Lowering deduplicates constants by tag and exact payload;
float payloads use bit equality, preserving signed zero and NaN payloads. The
verifier rejects out-of-range indices, duplicate pool entries and destination
representation mismatches. Semantic integer-range analysis reads the indexed
operand. Artifacts serialize indices and portable constants, never runtime handles.

`ScalarOperand::Constant { constant, ty }` also references this pool, retaining the
complete numeric `BuiltinType` rather than just a shared physical carrier. Validation
checks canonical pool tag, width/range and operation contract. For example, an i8
operand must fit i8 even though its pool carrier is `ConstantOperand::I32`; float
operands preserve exact f32/f64 bits. Unit, Boolean and string constants are not
numeric constant operands.

Compiler selection removes only eligible single-definition, block-local numeric
loads whose uses accept constants and whose debug states agree. It does not absorb
cross-block hoisted constants or mandate a literal/immediate byte encoding.

Runtime preparation embeds scalar payload bits in physical scalar operations.
For `BinaryOperands` and numeric `PredicateBranch`, immutable function-owned scalar
sites resolve bits, operand orientation, the existing shared numeric kernel and the
store/branch destination once. Execution does not look up the constant pool or
reimplement numeric semantics. Other predicate categories retain their ordinary
checked comparison path.

Strings are materialized lazily in a runtime-local cache owned by the loaded
module version; subsequent loads copy its compact Value. The GC traces these
cache entries with the program graph. Escaped rooted strings remain valid when
an obsolete cache is reclaimed. Independently loaded runtimes share immutable
verified code but have separate constant storage. Dynamic interning is not required.

This does not mean that `const` items produce heap-backed frozen objects.
Aggregate runtime values are built through explicit construction instructions.

## Worked Examples

The examples below show the execution shape.
Opcode spelling is implementation-defined when the semantic behavior is unchanged.
Literal operands shown on `LoadConst` are shorthand for constant-table indices.

### Example: Simple Arithmetic

Source:

```kagari
fn add(a: i32, b: i32) -> i32 {
    val c = a + b;
    c
}
```

The important semantic steps are:

1. read the admitted parameter homes for `a` and `b`
2. compute checked `a + b`
3. publish the result into `c` only on success
4. return the result

The bytecode shape is:

```text
# entry: arg0 -> s0, arg1 -> s1
Binary     s2, Add, s0, s1
Return     s2
```

In this shape:

- `s0` and `s1` are the explicitly mapped homes of `a` and `b`
- `s2` is the home of `c` in this illustrative selected form
- no separate local read/store opcodes or intermediate Boolean/transport values
  are required; a retained debug-state distinction may still require a `Move`

These are schematic execution shapes, not a promise of exact compiler instruction
counts or slot numbers. Direct destination selection preserves checked failure and
commit order, not the historical transport instruction sequence.

### Example: Conditional Control Flow

Source:

```kagari
fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}
```

The high-level `if` is lowered into explicit control-flow edges.

The bytecode shape is:

```text
# entry: arg0 -> s0, arg1 -> s1
PredicateBranch Gt, Slot(s0), Slot(s1), negated=false, L_then, L_else

L_then:
Move      s2, s0
Jump      L_join

L_else:
Move      s2, s1
Jump      L_join

L_join:
Return    s2
```

This example shows the key execution-model rule:

- high-level structured control flow becomes explicit `PredicateBranch`/`Branch`
  and `Jump` instructions
- the comparison remains `Gt`; no Boolean slot is published when its only use is
  the branch, and both incoming paths initialize the join result

### Example: Short-Circuit Boolean Logic

Source:

```kagari
fn test(a: bool, b: bool) -> bool {
    a && b
}
```

The bytecode must preserve short-circuit evaluation.
That means `b` must not be evaluated when `a` is already `false`.

The bytecode shape is:

```text
# entry: arg0 -> s0, arg1 -> s1
Branch    s0, L_rhs, L_false

L_false:
LoadConst s2, false
Jump      L_join

L_rhs:
Move      s2, s1
Jump      L_join

L_join:
Return    s2
```

This cannot be lowered to a single eager instruction like:

```text
AndAnd s2, s0, s1
```

because a single eager instruction would not preserve the required source-level short-circuit behavior.

## SSA and JIT Compatibility

Bytecode remains downstream of typed IR, not a replacement for it.

That keeps the architecture compatible with:

- future SSA construction
- optional optimization passes
- optional JIT backends

The architecture is:

- IR remains the place where CFG is explicit
- bytecode remains the interpreter-facing flattened format
- SSA and optimizing JIT work branches off from typed IR or SSA IR
- native backends consume verified MIR, sealed analyses and explicit ABI/link descriptions
- bytecode is never used to reconstruct native semantic input

## Future Work

The following artifact and implementation details are not fixed by this document:

- full opcode set
- final aggregate-value opcodes
- exact `CallTarget` encoding
- exact `PathId` and dynamic path operand encoding
- exact effect flag representation
- exact per-program-point root liveness beyond the function-wide conservative layout
- final module, type, host function, and path table layouts
- bytecode verification rules
- `.kbc` binary artifact encoding
- debug metadata and source mapping for bytecode instructions

These details are finalized as the typed IR, host path metadata, VM frame model, and module reload model stabilize.

Host calls use `CallTarget::Native(NativeImportId)` and the same ordinary native
import/declaration contract as standard and application callbacks. Verification
checks exact declaration identity, concrete arguments, callable requirements,
receiver access and semantic/physical signatures. External `TypeDeclaration`
schemas and explicit path requirements accompany the module. Loading checks them
against installed common modules and pins the selected entries to the loaded
version; diagnostic labels never select callbacks. Invocation retains effect,
cancellation, roots, scoped borrow and result validation.

`VerifiedProgram` checks a complete bytecode dependency closure before splitting
its modules into shared immutable code handles. Separate runtimes can link the
same handle without copying functions or layouts, while host bindings and module
instances remain runtime-owned. `Runtime::load_program` constructs the verified
handle for one runtime; `Runtime::load_verified_program` accepts a shared handle.
Direct loading and reload preflight enforce the artifact's in-memory count and
encoded-size limits before fingerprint computation.
LoadedModule is an immutable Arc-backed handle constructed by Runtime loading.
Clone and module queries share executable data. Public raw ModuleStore loading
and mutation of loaded bytecode are unavailable. Handles and linked host slots
carry registry ownership and reject another runtime, even when numeric module
keys or slots coincide. Complete dependency pinning and state reclamation still
belong to the remaining R10/R14 work.

Source nominal host types use `ValueType::HostHandle`, separate from `HeapObject`.
Opaque host call operands/results must match that representation; general equality
is invalid for it. Host identity remains in `Ty::Host` and the required host
declaration table, never in a display-string fallback. Signature-only references
and member reference closure participate in mandatory linking. This representation
does not enable storing host handles in default script-owned heap payloads.
Path table roots and `MakePathView` destinations also require `HostHandle`; path
tables carrying the former heap-object representation are rejected before execution.


## Native iteration instructions

Iter New accepts a concrete Array, Map, Set or String ABI and returns a GC iterator.
Iter Next accepts a concrete Iter<Item> ABI and returns Option<Item>. The verifier checks each ABI,
operand and result slot before linking/execution. Iter is a concrete ABI
type whose children participate in generic substitution, validation and layout
traversal. Runtime operations check handle ownership, generation, source revision
and item ABI; creation retains the source, captures and code version without
materializing a snapshot of the source items.
For loops use ordinary rooted cursor values and calls, without iteration scope
instructions. Structural writes invalidate the cursor; its next source read traps.
Custom Iterator/Iterable methods are ordinary static calls. The existing JIT
uses its normal interpreter fallback for unsupported iterator/control-flow bodies.
The semantic contract is in [builtins](builtins.md#iteration-protocols).

## Async execution contract

Future factories, Future/Task await and generic script spawn are implemented. The
[async executable contracts](../async-execution-design.md#executable-contracts)
define the checked handoff.

Portable metadata identifies the checked Future and Task roles, native producers, factory
and private resume-body signatures, output type and suspension capability. Await
encodes operand and typed destination; the following PC resumes only after output
initialization. Independent verification derives initialized live slots, rejects
direct resume-body calls and non-suspendable resources, and permits rooted cursors. Runtime also checks actual reachable mutable/host state.

MakeFuture records a local private resume target, all captured arguments and the
nominal Future type. Verification requires exact physical/semantic capture types,
the completed output and a suspendable target, with initialized capture slots.
It allocates cold state without invoking the target. A Task role is valid for Await
but never MakeFuture. Both awaitable roles require exactly one output parameter,
exact operand and destination semantic types, and a suspendable caller. The same
checked liveness and resource facts apply to both; accepting Task cannot bypass
scoped-host or mutation-guard checks.
