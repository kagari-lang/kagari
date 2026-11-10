//! Select complete scalar operations and input shapes from verified code once.
use crate::{
    module::execution::{
        ScalarSlot,
        instruction::ExecutionInstruction,
        layout::{FrameLayout, scalar_type},
    },
    numeric::binary_operation,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BinaryOp, BytecodeInstruction, JumpTarget, ScalarOperand, Slot, UnaryOp},
    module::{BytecodeFunction, BytecodeModule},
};
use kagari_contract::representation::builtin_representation;
use kagari_types::{
    integer::IntegerOp,
    payload::{
        comparison::ComparisonDomain,
        operation::{FloatOperation, FloatType, ScalarOperation, ShiftKind},
    },
    scalar::IntegerType,
};

#[derive(Clone, Copy)]
pub(super) enum ScalarInput {
    Slot(ScalarSlot),
    Constant(u64),
}

pub(super) enum ScalarTarget {
    Store(ScalarSlot),
    Branch {
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
}

fn binary(op: BinaryOp, representation: ValueType) -> Option<ScalarOperation> {
    match op {
        BinaryOp::Numeric(operation) => {
            ScalarOperation::integer(operation.op, operation.input, operation.rhs)
        }
        BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => None,
        _ => ScalarOperation::binary(binary_operation(op), scalar_type(representation)?),
    }
}

pub(super) fn prepare(
    instruction: &BytecodeInstruction,
    function: &BytecodeFunction,
    module: &BytecodeModule,
    layout: &FrameLayout,
) -> Option<ExecutionInstruction> {
    let slot = |slot: Slot| layout.location(slot.index())?.operand.scalar();
    let input = |operand: ScalarOperand| -> Option<ScalarInput> {
        Some(match operand {
            ScalarOperand::Slot(value) => ScalarInput::Slot(slot(value)?),
            ScalarOperand::Constant { constant, ty } => ScalarInput::Constant(
                module.constants[constant.index()]
                    .numeric_payload(ty)
                    .expect("verified numeric constant"),
            ),
        })
    };
    let representation = |operand: ScalarOperand| match operand {
        ScalarOperand::Slot(slot) => function.metadata.slots[slot.index()],
        ScalarOperand::Constant { ty, .. } => builtin_representation(ty),
    };
    let (operation, lhs, rhs, target) = match *instruction {
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => (
            binary(op, function.metadata.slots[lhs.index()])?,
            ScalarInput::Slot(slot(lhs)?),
            ScalarInput::Slot(slot(rhs)?),
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::BinaryOperands { dst, op, lhs, rhs } => (
            binary(op, representation(lhs))?,
            input(lhs)?,
            input(rhs)?,
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::PredicateBranch {
            op,
            lhs,
            rhs,
            negated,
            then_target,
            else_target,
        } => (
            binary(op, representation(lhs))?,
            input(lhs)?,
            input(rhs)?,
            ScalarTarget::Branch {
                then_target: if negated { else_target } else { then_target },
                else_target: if negated { then_target } else { else_target },
            },
        ),
        BytecodeInstruction::Numeric {
            dst,
            operation,
            lhs,
            rhs,
        } => (
            ScalarOperation::integer(operation.op, operation.input, operation.rhs)?,
            ScalarInput::Slot(slot(lhs)?),
            ScalarInput::Slot(slot(rhs.unwrap_or(lhs))?),
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::Unary { dst, op, operand } => (
            match op {
                UnaryOp::Neg => ScalarOperation::negation(scalar_type(
                    function.metadata.slots[operand.index()],
                )?)?,
                UnaryOp::Not => ScalarOperation::BoolNot,
            },
            ScalarInput::Slot(slot(operand)?),
            ScalarInput::Slot(slot(operand)?),
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::Convert {
            dst,
            src,
            conversion,
        } => (
            ScalarOperation::conversion(conversion.source, conversion.target)?,
            ScalarInput::Slot(slot(src)?),
            ScalarInput::Slot(slot(src)?),
            ScalarTarget::Store(slot(dst)?),
        ),
        _ => return None,
    };
    ExecutionInstruction::scalar_operation(operation, lhs, rhs, target)
}

impl ExecutionInstruction {
    pub(super) fn scalar_operation(
        operation: ScalarOperation,
        lhs: ScalarInput,
        rhs: ScalarInput,
        target: ScalarTarget,
    ) -> Option<Self> {
        match (operation, lhs, rhs, target) {
            // BEGIN GENERATED scalar-preparation
            // Source: codegen/scalar-instructions.toml. Regenerate with:
            // uv run python scripts/generate_scalar_execution.py
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8AddRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8SubRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8MulRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8DivRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8RemRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8BitAndRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8BitOrRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I8,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8BitXorRK {
                dst,
                lhs,
                rhs: rhs as i8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16AddRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16SubRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16MulRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16DivRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16RemRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16BitAndRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16BitOrRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I16,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16BitXorRK {
                dst,
                lhs,
                rhs: rhs as i16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32AddRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32SubRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32MulRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32DivRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32RemRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32BitAndRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32BitOrRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I32,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32BitXorRK {
                dst,
                lhs,
                rhs: rhs as i32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64AddRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64SubRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64MulRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64DivRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64RemRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64BitAndRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64BitOrRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::I64,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64BitXorRK {
                dst,
                lhs,
                rhs: rhs as i64,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8AddRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8SubRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8MulRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8DivRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8RemRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8BitAndRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8BitOrRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U8,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8BitXorRK {
                dst,
                lhs,
                rhs: rhs as u8,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16AddRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16SubRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16MulRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16DivRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16RemRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16BitAndRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16BitOrRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U16,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16BitXorRK {
                dst,
                lhs,
                rhs: rhs as u16,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32AddRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32SubRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32MulRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32DivRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32RemRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32BitAndRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32BitOrRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U32,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32BitXorRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedAdd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64AddRK { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedSub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64SubRK { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedMul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64MulRK { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedDiv,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64DivRK { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::CheckedRem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64RemRK { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64BitAndRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::BitAnd,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64BitAndRK { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64BitOrRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::BitOr,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64BitOrRK { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64BitXorRR { dst, lhs, rhs }),
            (
                ScalarOperation::Integer {
                    domain: IntegerType::U64,
                    op: IntegerOp::BitXor,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64BitXorRK { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Add,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Add,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32AddRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Sub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Sub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32SubRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Mul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Mul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32MulRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Div,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Div,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32DivRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Rem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F32,
                    op: FloatOperation::Rem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32RemRK {
                dst,
                lhs,
                rhs: rhs as u32,
            }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Add,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64AddRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Add,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64AddRK { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Sub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64SubRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Sub,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64SubRK { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Mul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64MulRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Mul,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64MulRK { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Div,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64DivRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Div,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64DivRK { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Rem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64RemRR { dst, lhs, rhs }),
            (
                ScalarOperation::Float {
                    domain: FloatType::F64,
                    op: FloatOperation::Rem,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64RemRK { dst, lhs, rhs }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I8Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I8Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I8Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I8Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I8ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I16Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I16Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I16Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I16Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I16ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I32Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I32Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I32Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I32Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I64Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I64Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I64Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::I64Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U8Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U8Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U8Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U8Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U8ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U16Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U16Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U16Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U16Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U16ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U32Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U32Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U32Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U32Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U32ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U64Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64ShlRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U64Shl,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64ShlRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U64Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64ShrRR {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Shift {
                    kind: ShiftKind::U64Shr,
                    count_domain,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64ShrRK {
                dst,
                lhs,
                rhs,
                count_domain,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32CompareRR {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I32CompareRK {
                dst,
                lhs,
                rhs: rhs as i32,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::I32CompareBranchRR {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::I32CompareBranchRK {
                lhs,
                rhs: rhs as i32,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64CompareRR {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::I64CompareRK {
                dst,
                lhs,
                rhs: rhs as i64,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::I64CompareBranchRR {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::I64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::I64CompareBranchRK {
                lhs,
                rhs: rhs as i64,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::U64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64CompareRR {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::U64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::U64CompareRK {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::U64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::U64CompareBranchRR {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::U64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::U64CompareBranchRK {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32CompareRR {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F32CompareRK {
                dst,
                lhs,
                rhs: rhs as u32,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::F32CompareBranchRR {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F32,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::F32CompareBranchRK {
                lhs,
                rhs: rhs as u32,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64CompareRR {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::F64CompareRK {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::F64CompareBranchRR {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::F64,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Constant(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::F64CompareBranchRK {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::Char,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::CharCompareRR {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::Char,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::CharCompareBranchRR {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::Scalar,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Store(dst),
            ) => Some(Self::ScalarCompareRR {
                dst,
                lhs,
                rhs,
                condition,
            }),
            (
                ScalarOperation::Compare {
                    domain: ComparisonDomain::Scalar,
                    condition,
                },
                ScalarInput::Slot(lhs),
                ScalarInput::Slot(rhs),
                ScalarTarget::Branch {
                    then_target,
                    else_target,
                },
            ) => Some(Self::ScalarCompareBranchRR {
                lhs,
                rhs,
                then_target,
                else_target,
                condition,
            }),
            (ScalarOperation::I8BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::I8BitNotR { dst, src })
            }
            (ScalarOperation::I16BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::I16BitNotR { dst, src })
            }
            (ScalarOperation::I32BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::I32BitNotR { dst, src })
            }
            (ScalarOperation::I64BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::I64BitNotR { dst, src })
            }
            (ScalarOperation::U8BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::U8BitNotR { dst, src })
            }
            (ScalarOperation::U16BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::U16BitNotR { dst, src })
            }
            (ScalarOperation::U32BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::U32BitNotR { dst, src })
            }
            (ScalarOperation::U64BitNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::U64BitNotR { dst, src })
            }
            (ScalarOperation::I32Neg, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::I32NegR { dst, src })
            }
            (ScalarOperation::I64Neg, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::I64NegR { dst, src })
            }
            (ScalarOperation::F32Neg, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::F32NegR { dst, src })
            }
            (ScalarOperation::F64Neg, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::F64NegR { dst, src })
            }
            (ScalarOperation::BoolNot, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => {
                Some(Self::BoolNotR { dst, src })
            }
            (
                ScalarOperation::IntegerConvert { source, target },
                ScalarInput::Slot(src),
                _,
                ScalarTarget::Store(dst),
            ) => Some(Self::IntegerConvert {
                dst,
                src,
                source,
                target,
            }),
            (
                ScalarOperation::IntegerToFloat { source, target },
                ScalarInput::Slot(src),
                _,
                ScalarTarget::Store(dst),
            ) => Some(Self::IntegerToFloat {
                dst,
                src,
                source,
                target,
            }),
            (
                ScalarOperation::FloatToInteger { source, target },
                ScalarInput::Slot(src),
                _,
                ScalarTarget::Store(dst),
            ) => Some(Self::FloatToInteger {
                dst,
                src,
                source,
                target,
            }),
            (
                ScalarOperation::FloatConvert { source, target },
                ScalarInput::Slot(src),
                _,
                ScalarTarget::Store(dst),
            ) => Some(Self::FloatConvert {
                dst,
                src,
                source,
                target,
            }),
            // END GENERATED scalar-preparation
            (
                ScalarOperation::BoolToInteger { target },
                ScalarInput::Slot(src),
                _,
                ScalarTarget::Store(dst),
            ) => Some(Self::BoolToInteger { dst, src, target }),
            (operation, lhs, rhs, target) => prepare_shared(operation, lhs, rhs, target),
        }
    }
}

fn prepare_shared(
    operation: ScalarOperation,
    lhs: ScalarInput,
    rhs: ScalarInput,
    target: ScalarTarget,
) -> Option<ExecutionInstruction> {
    let ScalarInput::Constant(lhs) = lhs else {
        return None;
    };
    macro_rules! store {
        ($kr:ident, $kk:ident, $dst:ident, $($field:ident),+) => {
            Some(match rhs {
                ScalarInput::Slot(rhs) => ExecutionInstruction::$kr { dst: $dst, lhs, rhs, $($field),+ },
                ScalarInput::Constant(rhs) => ExecutionInstruction::$kk { dst: $dst, lhs, rhs, $($field),+ },
            })
        };
    }
    match (operation, target) {
        (ScalarOperation::Integer { domain, op }, ScalarTarget::Store(dst)) => {
            store!(IntegerBinaryKR, IntegerBinaryKK, dst, domain, op)
        }
        (ScalarOperation::Float { domain, op }, ScalarTarget::Store(dst)) => {
            store!(FloatBinaryKR, FloatBinaryKK, dst, domain, op)
        }
        (ScalarOperation::Shift { kind, count_domain }, ScalarTarget::Store(dst)) => {
            store!(ShiftKR, ShiftKK, dst, kind, count_domain)
        }
        (ScalarOperation::Compare { domain, condition }, ScalarTarget::Store(dst)) => {
            store!(CompareStoreKR, CompareStoreKK, dst, domain, condition)
        }
        (
            ScalarOperation::Compare { domain, condition },
            ScalarTarget::Branch {
                then_target,
                else_target,
            },
        ) => Some(match rhs {
            ScalarInput::Slot(rhs) => ExecutionInstruction::CompareBranchKR {
                lhs,
                rhs,
                domain,
                condition,
                then_target,
                else_target,
            },
            // This pure predicate cannot trap after constant admission. Keep its PC.
            ScalarInput::Constant(rhs) => ExecutionInstruction::Jump(
                if domain
                    .evaluate(condition, lhs, rhs)
                    .expect("verified comparison constants")
                    == 1
                {
                    then_target
                } else {
                    else_target
                },
            ),
        }),
        _ => None,
    }
}
