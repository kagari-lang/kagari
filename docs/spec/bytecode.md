# Kagari Bytecode

This document specifies the Kagari bytecode execution model.
The on-disk artifact encoding is versioned separately from the semantic bytecode model described here.

Execution-model context is defined in [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md).
Backend abstraction rules are defined in [codegen-backend.md](/Users/mikai/CLionProjects/kagari/docs/spec/codegen-backend.md).
Typed path mutation rules are defined in [typed-path-mutation.md](/Users/mikai/CLionProjects/kagari/docs/spec/typed-path-mutation.md).

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
4. construction IR
5. bytecode
6. VM execution

Optional backend paths include:

- construction IR -> SSA IR -> optimized backend
- construction IR -> JIT backend

Bytecode remains a first-class execution format when those backend paths are present.

## Execution Model

Kagari bytecode is register/local based.
Kagari bytecode is not a stack-machine format.

That means:

- temporary expression results live in virtual registers
- user-visible variable storage lives in local slots
- control flow is expressed through explicit jump and branch instructions

This matches the current IR model closely and keeps `IR -> bytecode` lowering simple.
It also keeps data dependencies visible enough for verification, effect analysis, and JIT lowering.

The bytecode register model is virtual.
The interpreter stores registers in VM frames; machine-code backends map the same virtual register semantics to backend-specific locations.
Source-visible variable semantics remain attached to locals, not to bytecode registers.

## Module Layout

The module model is:

```text
BytecodeModule {
  module_id: ModuleId,
  epoch: ModuleEpoch,
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
  id: FunctionId,
  name: String,
  parameter_count: u16,
  register_count: u16,
  local_count: u16,
  instructions: [BytecodeInstruction],
  metadata: FunctionMetadata
}
```

Function metadata includes:

- source span table, if available
- parameter and return layout
- local and register type layout, if retained for verification or JIT
- effect summary
- safepoint metadata
- public ABI fingerprint, when public

## Implementation Status

The current in-memory Rust shape is:

```text
BytecodeModule {
  module_init: Option<FunctionRef>,
  module_slots: [BytecodeModuleSlot],
  functions: [BytecodeFunction]
}
```

Each current function contains:

```text
BytecodeFunction {
  id: FunctionRef,
  name: String,
  parameter_count: u16,
  register_count: u16,
  local_count: u16,
  instructions: [BytecodeInstruction]
}
```

This implementation slice is valid for the current interpreter, but it is not the bytecode artifact contract.

## Operand Model

Bytecode operands are compact index-like values.
The operand model includes:

- `Register`
- `LocalSlot`
- `ModuleSlot`
- `JumpTarget`
- `ConstId` or inline `ConstantOperand`
- `CallTarget`
- `FunctionRef`
- `HostFunctionId`
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
- `LoadLocal`
- `StoreLocal`
- `Move`
- unary arithmetic and logical operations
- binary arithmetic and comparison operations
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
LoadLocal dst, local
StoreLocal local, src
LoadModule dst, slot
StoreModule slot, src
Move dst, src

Unary dst, op, operand
Binary dst, op, lhs, rhs

Call dst?, callee, args
Jump target
Branch cond, then_target, else_target
Return value?
Unreachable
```

This shape matches the register/local execution model and keeps control flow explicit.

## Aggregate Instructions

Bytecode distinguishes ordinary script-owned aggregate access from host-backed path access.

Ordinary aggregate instructions:

```text
MakeTuple dst, elements
MakeArray dst, elements
MakeStruct dst, type_id, field_values
ReadField dst, base, field_id
WriteField base, field_id, value
ReadIndex dst, base, index
WriteIndex base, index, value
```

These instructions apply to script-owned values or runtime-managed aggregates.
They must not silently become reflection over host-owned Rust state.

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
- path operations can call host code, trap, check capabilities, validate epochs, and mark dirty state

Example source:

```kagari
player.inventory.items[item_id].count -= 1
```

Bytecode shape:

```text
LoadLocal    r0, local_player
LoadLocal    r1, local_item_id
LoadConst    r2, 1
ModifyPath   _, r0, path_Player_inventory_items_count, [r1], SubAssign, r2
```

Example with a local path view:

```kagari
val combat = player.combat
combat.hp -= damage
```

Bytecode shape:

```text
LoadLocal     r0, local_player
MakePathView  r1, r0, path_Player_combat, []
StoreLocal    local_combat, r1

LoadLocal     r2, local_combat
LoadLocal     r3, local_damage
ModifyPath    _, r2, path_Combat_hp, [], SubAssign, r3
```

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
- indirect register-based calls
- host function calls
- runtime helper calls
- builtin method calls
- interface dispatch helper calls

The `CallTarget` model records this distinction.

Host calls and runtime helpers remain explicit because they can:

- trap
- allocate
- trigger capability checks
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
- `may_check_capability`
- `may_access_reflection`
- `may_mutate_script_heap`
- `may_mutate_host_state`
- `may_mark_dirty`
- `may_suspend`
- `is_safepoint`

Effect classification:

```text
LoadLocal       no observable effect
Binary          may_trap for checked arithmetic or invalid operands
MakeArray       may_allocate, is_safepoint
Call host       may_call_host, may_trap, may_check_capability, is_safepoint
ReadPath        may_call_host, may_trap, may_check_capability
SetPath         may_call_host, may_trap, may_check_capability, may_mutate_host_state, may_mark_dirty
ModifyPath      may_call_host, may_trap, may_check_capability, may_mutate_host_state, may_mark_dirty
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

- register indices are in range
- local indices are in range
- jump targets point to valid instruction boundaries
- functions return values compatible with their declared return type
- instruction operands match the expected type layout when type metadata is retained
- local and field writes respect `val` / `var` writeability
- `PathId` operands exist in the path table
- path dynamic argument counts match the descriptor
- path result values are used with compatible types
- write operations target writable paths
- public function signatures are concrete and ABI-supported

Verification rejects malformed bytecode before execution.
Runtime checks are still required for host object liveness, dynamic index validity, capability state, and host-side invariants.

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

Bytecode preserves enough value-liveness information for precise GC.

GC and safepoint metadata includes:

- which registers and locals contain GC-managed values at safepoints
- which registers and locals contain host handles or path views
- which values are ephemeral and must not cross suspension points
- call boundary metadata for host calls and runtime helpers

Machine-code backends require equivalent stack maps or root maps.
Bytecode must not hide value liveness or helper boundaries that those backends need for equivalent root maps.

## Debug Metadata

Bytecode supports source mapping.

Debug metadata includes:

- instruction-to-source spans
- local variable names and live ranges
- function names and module names
- path names for diagnostics
- type names for diagnostics

Debug metadata is not required for execution correctness.
It is removable or compactable for production artifacts.

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

- [bytecode/mod.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/mod.rs)
- [bytecode/module.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/module.rs)
- [bytecode/instruction.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/instruction.rs)
- [bytecode/lower.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/lower.rs)

The current implementation defines the execution-layer boundary while the serialized artifact format remains separate.

## Relationship to IR

Bytecode is lower than construction IR and more execution-oriented.

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

The `JumpTarget` representation used by the VM is instruction-stream based, not block-id based.

## Constant Handling

The current `ConstantOperand` model is small and inline.

For the current language model, `LoadConst` is primarily aimed at small scalar operands.

Future Work: string-heavy or other large inline operands can move into a constant pool when:

- serialized artifact size matters
- deduplication becomes worthwhile
- VM loading cost needs to be reduced

This does not mean that `const` items produce heap-backed frozen objects.
Aggregate runtime values are built through explicit construction instructions.

## Worked Examples

The examples below show the execution shape.
Opcode spelling is implementation-defined when the semantic behavior is unchanged.

### Example: Simple Arithmetic

Source:

```kagari
fn add(a: i32, b: i32) -> i32 {
    val c = a + b;
    c
}
```

The important semantic steps are:

1. read `a` from a local slot
2. read `b` from a local slot
3. compute `a + b`
4. write the result into local `c`
5. read `c`
6. return it

The bytecode shape is:

```text
LoadLocal  r0, local0
LoadLocal  r1, local1
Add        r2, r0, r1
StoreLocal local2, r2
LoadLocal  r3, local2
Return     r3
```

In this shape:

- `local0` is `a`
- `local1` is `b`
- `local2` is `c`
- `r0..r3` are virtual registers for intermediate execution results

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
LoadLocal r0, local0
LoadLocal r1, local1
Gt        r2, r0, r1
Branch    r2, L_then, L_else

L_then:
LoadLocal r3, local0
Move      r5, r3
Jump      L_join

L_else:
LoadLocal r4, local1
Move      r5, r4
Jump      L_join

L_join:
Return    r5
```

This example shows the key execution-model rule:

- high-level structured control flow becomes explicit `Branch` and `Jump` instructions

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
LoadLocal r0, local0
Branch    r0, L_rhs, L_false

L_false:
LoadConst r2, false
Jump      L_join

L_rhs:
LoadLocal r1, local1
Move      r2, r1
Jump      L_join

L_join:
Return    r2
```

This cannot be lowered to a single eager instruction like:

```text
AndAnd r2, r0, r1
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
- baseline JIT backends can consume bytecode or a bytecode-like lowered form if they preserve the same helper and metadata contracts

## Future Work

The following artifact and implementation details are not fixed by this document:

- full opcode set
- final aggregate-value opcodes
- exact `CallTarget` encoding
- exact `PathId` and dynamic path operand encoding
- exact effect flag representation
- exact safepoint and root-map metadata representation
- final module, type, host function, and path table layouts
- bytecode verification rules
- `.kbc` binary artifact encoding
- debug metadata and source mapping for bytecode instructions

These details are finalized as the typed IR, host path metadata, VM frame model, and module reload model stabilize.
