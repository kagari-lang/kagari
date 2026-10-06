//! Compact physical operations derived once from sealed, verified bytecode.
//! Logical PCs remain one-to-one with the canonical code. Identity-bearing and
//! variable-length operands stay in that immutable code, addressed by the PC.
use kagari_bytecode::{
    instruction::{
        BinaryOp, BytecodeInstruction, ConstantOperand, JumpTarget, LocalSlot, Register, UnaryOp,
    },
    module::BytecodeModule,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::numeric::{NumericConversion, NumericOperation};

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

#[derive(Debug, Clone, Copy)]
pub enum ExecutionInstruction {
    Constant {
        dst: Register,
        value: ScalarConstant,
    },
    LoadLocal {
        dst: Register,
        local: LocalSlot,
    },
    StoreLocal {
        local: LocalSlot,
        src: Register,
    },
    Move {
        dst: Register,
        src: Register,
    },
    Unary {
        dst: Register,
        op: UnaryOp,
        operand: Register,
    },
    Binary {
        dst: Register,
        op: BinaryOp,
        lhs: Register,
        rhs: Register,
    },
    Convert {
        dst: Register,
        src: Register,
        conversion: NumericConversion,
    },
    Numeric {
        dst: Register,
        operation: NumericOperation,
        lhs: Register,
        rhs: Option<Register>,
    },
    Jump(JumpTarget),
    Branch {
        cond: Register,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Return(Option<Register>),
    /// Consult the canonical instruction at the current logical PC after
    /// releasing the cursor, before a potentially allocating/reentrant operation.
    Boundary,
}

#[derive(Debug)]
pub(crate) struct ExecutionModule {
    pub functions: Vec<Box<[ExecutionInstruction]>>,
}

impl ExecutionModule {
    // Only VerifiedProgram constructs this product, after artifact verification.
    // Identity normalization preserves instruction order and physical operands,
    // so the product can be shared across runtime-local definition scopes.
    pub(super) fn prepare(module: &BytecodeModule<DefinitionId>) -> Self {
        Self {
            functions: module
                .functions
                .iter()
                .map(|function| {
                    function
                        .instructions
                        .iter()
                        .map(ExecutionInstruction::prepare)
                        .collect()
                })
                .collect(),
        }
    }
}

impl ExecutionInstruction {
    fn prepare(instruction: &BytecodeInstruction<DefinitionId>) -> Self {
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
                Self::Constant { dst, value }
            }
            BytecodeInstruction::LoadLocal { dst, local } => Self::LoadLocal { dst, local },
            BytecodeInstruction::StoreLocal { local, src } => Self::StoreLocal { local, src },
            BytecodeInstruction::Move { dst, src } => Self::Move { dst, src },
            BytecodeInstruction::Unary { dst, op, operand } => Self::Unary { dst, op, operand },
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => Self::Binary { dst, op, lhs, rhs },
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => Self::Convert {
                dst,
                src,
                conversion,
            },
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => Self::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            },
            BytecodeInstruction::Jump { target } => Self::Jump(target),
            BytecodeInstruction::Branch {
                cond,
                then_target,
                else_target,
            } => Self::Branch {
                cond,
                then_target,
                else_target,
            },
            BytecodeInstruction::Return(value) => Self::Return(value),
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
