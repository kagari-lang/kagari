//! Compact physical operations derived once from sealed, verified bytecode.
//! Logical PCs remain one-to-one with the canonical code. Identity-bearing and
//! variable-length operands stay in that immutable code, addressed by the PC.
pub(crate) mod allocation;
mod operands;

use crate::module::execution::allocation::RegisterAllocation;
use kagari_bytecode::{
    instruction::{BinaryOp, BytecodeInstruction, ConstantOperand, JumpTarget, Register, UnaryOp},
    module::BytecodeModule,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::numeric::{NumericConversion, NumericOperation};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub enum ScalarConstant {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
}

/// A bounded physical operand in a prepared function's value window.
#[derive(Debug, Clone, Copy)]
pub struct OperandSlot(u32);

impl OperandSlot {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ExecutionInstruction {
    Constant {
        dst: OperandSlot,
        value: ScalarConstant,
    },
    Move {
        dst: OperandSlot,
        src: OperandSlot,
    },
    Unary {
        dst: OperandSlot,
        op: UnaryOp,
        operand: OperandSlot,
    },
    Binary {
        dst: OperandSlot,
        op: BinaryOp,
        lhs: OperandSlot,
        rhs: OperandSlot,
    },
    Convert {
        dst: OperandSlot,
        src: OperandSlot,
        conversion: NumericConversion,
    },
    Numeric {
        dst: OperandSlot,
        operation: NumericOperation,
        lhs: OperandSlot,
        rhs: Option<OperandSlot>,
    },
    Jump(JumpTarget),
    Branch {
        cond: OperandSlot,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Return(Option<OperandSlot>),
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
    pub registers: Arc<RegisterAllocation>,
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
                    let registers = Arc::new(RegisterAllocation::prepare(function, work));
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
    fn prepare(
        instruction: &BytecodeInstruction<DefinitionId>,
        registers: &RegisterAllocation,
    ) -> Self {
        let slot = |register: Register| {
            OperandSlot(
                registers
                    .index(register.index())
                    .expect("verified register") as u32,
            )
        };

        match *instruction {
            BytecodeInstruction::LoadConst { dst, ref constant } => {
                let value = match *constant {
                    ConstantOperand::Unit => ScalarConstant::Unit,
                    ConstantOperand::Bool(v) => ScalarConstant::Bool(v),
                    ConstantOperand::I32(v) => ScalarConstant::I32(v),
                    ConstantOperand::I64(v) => ScalarConstant::I64(v),
                    ConstantOperand::U64(v) => ScalarConstant::U64(v),
                    ConstantOperand::F32(v) => ScalarConstant::F32(v),
                    ConstantOperand::F64(v) => ScalarConstant::F64(v),
                    ConstantOperand::Str(_) => return Self::Boundary,
                };
                Self::Constant {
                    dst: slot(dst),
                    value,
                }
            }
            BytecodeInstruction::LoadLocal { dst, local } => Self::Move {
                dst: slot(dst),
                src: OperandSlot((registers.count + local.index()) as u32),
            },
            BytecodeInstruction::StoreLocal { local, src } => Self::Move {
                dst: OperandSlot((registers.count + local.index()) as u32),
                src: slot(src),
            },
            BytecodeInstruction::Move { dst, src } => Self::Move {
                dst: slot(dst),
                src: slot(src),
            },
            BytecodeInstruction::Unary { dst, op, operand } => Self::Unary {
                dst: slot(dst),
                op,
                operand: slot(operand),
            },
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => Self::Binary {
                dst: slot(dst),
                op,
                lhs: slot(lhs),
                rhs: slot(rhs),
            },
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => Self::Convert {
                dst: slot(dst),
                src: slot(src),
                conversion,
            },
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => Self::Numeric {
                dst: slot(dst),
                operation,
                lhs: slot(lhs),
                rhs: rhs.map(slot),
            },
            BytecodeInstruction::Jump { target } => Self::Jump(target),
            BytecodeInstruction::Branch {
                cond,
                then_target,
                else_target,
            } => Self::Branch {
                cond: slot(cond),
                then_target,
                else_target,
            },
            BytecodeInstruction::Return(value) => Self::Return(value.map(slot)),
            _ => Self::Boundary,
        }
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
