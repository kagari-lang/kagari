# Kagari Bytecode Draft

This document describes the current direction for Kagari bytecode.
It is a design draft, not a finalized binary compatibility contract.

Execution-model context is drafted in [execution.md](/Users/mikai/CLionProjects/kagari/docs/spec/execution.md).
Backend abstraction direction is drafted in [codegen-backend.md](/Users/mikai/CLionProjects/kagari/docs/spec/codegen-backend.md).

## Scope

This document defines:

- the semantic execution shape of bytecode
- the in-memory Rust model used by the compiler and VM layers
- the lowering boundary from IR into bytecode

This document does not yet define:

- the final on-disk `.kbc` binary encoding
- versioning and compatibility rules for serialized artifacts
- bytecode verification rules in full detail

## Design Goals

- keep bytecode as the primary interpreter target
- preserve a clean lowering path from non-SSA IR
- avoid coupling bytecode directly to AST or HIR structure
- keep the format compatible with future SSA and JIT work
- make VM execution straightforward and predictable

## Position in the Pipeline

The intended lowering pipeline is:

1. source
2. syntax
3. HIR
4. construction IR
5. bytecode
6. VM execution

Optional later paths may include:

- construction IR -> SSA IR -> optimized backend
- construction IR -> JIT backend

Bytecode remains a first-class execution format even if those later paths are added.

## Execution Model

The current direction is register/local based bytecode.

That means:

- temporary expression results live in virtual registers
- user-visible variable storage lives in local slots
- control flow is expressed through explicit jump and branch instructions

This matches the current IR model closely and keeps `IR -> bytecode` lowering simple.

## Module Layout

Conceptually:

```text
BytecodeModule {
  functions: [BytecodeFunction]
}
```

Each function contains:

```text
BytecodeFunction {
  name: String,
  instructions: [BytecodeInstruction]
}
```

Later revisions are expected to add:

- register count
- local slot count
- constant pool references
- function metadata
- module id / epoch metadata

## Operand Model

The current bytecode layer uses these logical operand kinds:

- `Register`
- `LocalSlot`
- `JumpTarget`
- `ConstantOperand`
- `CallTarget`

These are index-like values in the in-memory model and should remain cheap to lower from IR.

## Minimum Instruction Set

The first bytecode instruction set should cover:

- `LoadConst`
- `LoadLocal`
- `StoreLocal`
- `Move`
- `Call`
- `Jump`
- `Branch`
- `Return`
- `Unreachable`

Later instruction families will include:

- unary arithmetic and logical operations
- binary arithmetic and comparison operations
- aggregate construction
- aggregate access
- runtime helper calls when needed

## Current In-Memory Shape

The current code shape lives in:

- [bytecode/mod.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/mod.rs)
- [bytecode/module.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/module.rs)
- [bytecode/instruction.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/instruction.rs)
- [bytecode/lower.rs](/Users/mikai/CLionProjects/kagari/crates/kagari-ir/src/bytecode/lower.rs)

The current shape is intentionally still small.
It is meant to define the execution layer boundary before the final VM format is filled in.

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

This means bytecode should not become the only internal execution representation.
IR remains the better place for control-flow construction and future optimization work.

## Block Flattening

IR uses basic blocks.
Bytecode uses a linear instruction stream.

The lowering rule should be:

- choose a block order
- emit each block's instructions into a flat instruction list
- map each `BlockId` to a `JumpTarget`
- rewrite IR terminators into bytecode jumps and branches

The final `JumpTarget` representation used by the VM should be instruction-stream based, not block-id based.

## Constant Handling

The current `ConstantOperand` model is small and inline.

That is acceptable for the current stage.

For the current language model, `LoadConst` is primarily aimed at small scalar operands.

Later revisions may move string-heavy or other large inline operands into a constant pool if:

- serialized artifact size matters
- deduplication becomes worthwhile
- VM loading cost needs to be reduced

This should not be read as a commitment that `const` items produce heap-backed frozen objects.
Aggregate runtime values are still expected to be built through explicit construction instructions unless the runtime model is later extended.

## Worked Examples

The examples below are illustrative.
They show the intended execution shape, not a frozen final opcode contract.

### Example: Simple Arithmetic

Source:

```kagari
fn add(a: i32, b: i32) -> i32 {
    let c = a + b;
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

A representative bytecode shape is:

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

A representative bytecode shape is:

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

A representative bytecode shape is:

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

This is preferable to a naive single instruction like:

```text
AndAnd r2, r0, r1
```

because a single eager instruction would not preserve the required source-level short-circuit behavior.

## Calls

Calls currently distinguish between:

- direct function calls
- indirect register-based calls

This is enough for the current IR lowering direction.

Later revisions may add:

- host-call specific bytecode
- helper-call specific bytecode
- trait/dynamic dispatch helper calls

## Runtime Interaction

Bytecode should not encode complex runtime policy directly.

Operations involving:

- allocation
- host interop
- reflection
- capability enforcement
- dynamic dispatch helpers

should continue to lower through explicit helper-oriented execution paths.

This keeps interpreter semantics and future JIT semantics aligned.

## SSA and JIT Compatibility

The bytecode design should remain downstream of IR, not upstream of it.

That keeps the architecture compatible with:

- future SSA construction
- optional optimization passes
- optional JIT backends

The intended direction is:

- IR remains the place where CFG is explicit
- bytecode remains the interpreter-facing flattened format
- SSA and JIT work branch off from IR, not from bytecode

## What Is Still Intentionally Open

The following are not yet fixed:

- full opcode set
- final aggregate-value opcodes
- exact `CallTarget` encoding
- bytecode verification rules
- `.kbc` binary artifact encoding
- debug metadata and source mapping for bytecode instructions

Those should be finalized after the current `IR -> bytecode` lowering path is fully implemented for the existing syntax subset.
