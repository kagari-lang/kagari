//! Compact physical operations derived once from sealed, verified bytecode.
//! Logical PCs remain one-to-one with the canonical code. Identity-bearing and
//! variable-length operands stay in that immutable code, addressed by the PC.
pub(crate) mod allocation;
pub(crate) mod layout;
mod operands;

use crate::{
    frame::values::scalar,
    module::execution::layout::{FrameLayout, scalar_type},
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BinaryOp, BytecodeInstruction, ConstantOperand, JumpTarget, Register, UnaryOp},
    module::BytecodeModule,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::payload::{self, ScalarBinaryOp, ScalarKernel};
use std::sync::Arc;

/// A bounded physical operand in a prepared function's value window.
#[derive(Debug, Clone, Copy)]
pub struct OperandSlot(u32);

impl OperandSlot {
    pub(crate) fn new(index: usize, managed: bool) -> Self {
        // Sealed register/local counts are u16; this bit only classifies frame
        // offsets and never truncates a runtime owner/slot/generation identity.
        Self(index as u32 | if managed { 1 << 31 } else { 0 })
    }

    pub(crate) fn index(self) -> usize {
        (self.0 & !(1 << 31)) as usize
    }

    pub(crate) fn managed(self) -> bool {
        self.0 & (1 << 31) != 0
    }

    pub(crate) fn scalar(self) -> Option<ScalarSlot> {
        (!self.managed()).then_some(ScalarSlot(self.0))
    }
}

/// A sealed scalar-bank offset. General locations prove their class once during
/// preparation; scalar execution cannot reinterpret a managed offset as payload.
#[derive(Debug, Clone, Copy)]
pub struct ScalarSlot(u32);

impl ScalarSlot {
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ExecutionInstruction {
    Constant {
        dst: ScalarSlot,
        value: u64,
    },
    Move {
        dst: ScalarSlot,
        src: ScalarSlot,
    },
    Scalar {
        dst: ScalarSlot,
        lhs: ScalarSlot,
        rhs: ScalarSlot,
        kernel: ScalarKernel,
    },
    Jump(JumpTarget),
    Branch {
        cond: ScalarSlot,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Return {
        value: Option<OperandSlot>,
        representation: ValueType,
    },
    /// Consult the canonical instruction at the current logical PC after
    /// releasing the cursor, before a potentially allocating/reentrant operation.
    Boundary,
}

#[derive(Debug)]
pub(crate) struct ExecutionModule {
    pub functions: Vec<ExecutionFunction>,
}

#[derive(Debug)]
pub(crate) struct ExecutionFunction {
    pub instructions: Box<[ExecutionInstruction]>,
    pub registers: Arc<FrameLayout>,
}

impl ExecutionModule {
    // Only VerifiedProgram constructs this product, after artifact verification.
    // Identity normalization preserves instruction order and physical operands,
    // so the product can be shared across runtime-local definition scopes.
    pub(super) fn prepare(module: &BytecodeModule<DefinitionId>, work: &mut usize) -> Self {
        Self {
            functions: module
                .functions
                .iter()
                .map(|function| {
                    let registers = Arc::new(FrameLayout::prepare(function, work));
                    let instructions = function
                        .instructions
                        .iter()
                        .map(|instruction| ExecutionInstruction::prepare(instruction, &registers))
                        .collect();
                    ExecutionFunction {
                        instructions,
                        registers,
                    }
                })
                .collect(),
        }
    }
}

impl ExecutionInstruction {
    fn prepare(instruction: &BytecodeInstruction<DefinitionId>, registers: &FrameLayout) -> Self {
        let location = |register: Register| {
            registers
                .location(register.index())
                .expect("verified register")
        };
        let slot = |register: Register| location(register).operand;
        let local_slot = |index: usize| {
            registers
                .location(registers.register_count + index)
                .expect("verified local")
                .operand
        };
        let scalar = |dst: Register, lhs: Register, rhs: Register, kernel: Option<ScalarKernel>| {
            let Some(kernel) = kernel else {
                return Self::Boundary;
            };
            let (Some(dst), Some(lhs), Some(rhs)) =
                (slot(dst).scalar(), slot(lhs).scalar(), slot(rhs).scalar())
            else {
                return Self::Boundary;
            };
            Self::Scalar {
                dst,
                lhs,
                rhs,
                kernel,
            }
        };

        match *instruction {
            BytecodeInstruction::LoadConst { dst, ref constant } => {
                let value = match *constant {
                    ConstantOperand::Unit => Value::Unit,
                    ConstantOperand::Bool(v) => Value::Bool(v),
                    ConstantOperand::I32(v) => Value::I32(v),
                    ConstantOperand::I64(v) => Value::I64(v),
                    ConstantOperand::U64(v) => Value::U64(v),
                    ConstantOperand::F32(v) => Value::F32(v),
                    ConstantOperand::F64(v) => Value::F64(v),
                    ConstantOperand::Str(_) => return Self::Boundary,
                };
                let Some(dst) = slot(dst).scalar() else {
                    return Self::Boundary;
                };
                Self::Constant {
                    dst,
                    value: scalar::encode(&value).expect("scalar constant"),
                }
            }
            BytecodeInstruction::LoadLocal { dst, local } => {
                Self::move_slots(slot(dst), local_slot(local.index()))
            }
            BytecodeInstruction::StoreLocal { local, src } => {
                Self::move_slots(local_slot(local.index()), slot(src))
            }
            BytecodeInstruction::Move { dst, src } => Self::move_slots(slot(dst), slot(src)),
            BytecodeInstruction::Unary { dst, op, operand } => {
                let ty = scalar_type(location(operand).representation);
                let kernel = match op {
                    UnaryOp::Neg => ty.and_then(payload::negation_kernel),
                    UnaryOp::Not => Some(payload::boolean_not as ScalarKernel),
                };
                scalar(dst, operand, operand, kernel)
            }
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
                let kernel = match op {
                    BinaryOp::Numeric(operation) => {
                        payload::integer_kernel(operation.op, operation.input, operation.rhs)
                    }
                    BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => None,
                    _ => scalar_type(location(lhs).representation)
                        .and_then(|ty| payload::binary_kernel(binary_op(op), ty)),
                };
                scalar(dst, lhs, rhs, kernel)
            }
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => scalar(
                dst,
                src,
                src,
                payload::conversion_kernel(conversion.source, conversion.target),
            ),
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => scalar(
                dst,
                lhs,
                rhs.unwrap_or(lhs),
                payload::integer_kernel(operation.op, operation.input, operation.rhs),
            ),
            BytecodeInstruction::Jump { target } => Self::Jump(target),
            BytecodeInstruction::Branch {
                cond,
                then_target,
                else_target,
            } => Self::Branch {
                cond: slot(cond).scalar().expect("verified bool location"),
                then_target,
                else_target,
            },
            BytecodeInstruction::Return(value) => Self::Return {
                value: value.map(slot),
                representation: value.map_or(ValueType::Unit, |register| {
                    location(register).representation
                }),
            },
            _ => Self::Boundary,
        }
    }

    fn move_slots(dst: OperandSlot, src: OperandSlot) -> Self {
        match (dst.scalar(), src.scalar()) {
            (Some(dst), Some(src)) => Self::Move { dst, src },
            _ => Self::Boundary,
        }
    }
}

fn binary_op(op: BinaryOp) -> ScalarBinaryOp {
    match op {
        BinaryOp::Add => ScalarBinaryOp::Add,
        BinaryOp::Sub => ScalarBinaryOp::Sub,
        BinaryOp::Mul => ScalarBinaryOp::Mul,
        BinaryOp::Div => ScalarBinaryOp::Div,
        BinaryOp::Rem => ScalarBinaryOp::Rem,
        BinaryOp::Eq => ScalarBinaryOp::Eq,
        BinaryOp::NotEq => ScalarBinaryOp::NotEq,
        BinaryOp::Lt => ScalarBinaryOp::Lt,
        BinaryOp::Le => ScalarBinaryOp::Le,
        BinaryOp::Gt => ScalarBinaryOp::Gt,
        BinaryOp::Ge => ScalarBinaryOp::Ge,
        BinaryOp::Numeric(_) | BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn physical_instruction_budget() {
        assert!(size_of::<ExecutionInstruction>() <= 24);
    }
}
