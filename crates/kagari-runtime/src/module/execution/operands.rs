//! Prepared canonical numeric operands; no constant lookup or type selection at execution.
use crate::{
    module::execution::{
        ScalarSlot,
        layout::{FrameLayout, scalar_type},
    },
    numeric::binary_operation,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BinaryOp, BytecodeInstruction, JumpTarget, ScalarOperand},
    module::{BytecodeFunction, BytecodeModule},
};
use kagari_contract::representation::builtin_representation;
use kagari_types::payload::{self, ScalarKernel};

#[derive(Debug, Clone, Copy)]
pub(crate) enum PreparedOperands {
    Slots(ScalarSlot, ScalarSlot),
    SlotConstant {
        slot: ScalarSlot,
        constant: u64,
        constant_left: bool,
    },
    Constants(u64, u64),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ScalarDestination {
    Store(ScalarSlot),
    Branch {
        negated: bool,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PreparedScalarSite {
    pub operands: PreparedOperands,
    pub kernel: ScalarKernel,
    pub destination: ScalarDestination,
}

pub(super) fn binary_kernel(op: BinaryOp, representation: ValueType) -> Option<ScalarKernel> {
    match op {
        BinaryOp::Numeric(operation) => {
            payload::integer_kernel(operation.op, operation.input, operation.rhs)
        }
        BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => None,
        _ => scalar_type(representation)
            .and_then(|ty| payload::binary_kernel(binary_operation(op), ty)),
    }
}

impl PreparedScalarSite {
    pub(super) fn prepare(
        instruction: &BytecodeInstruction,
        function: &BytecodeFunction,
        module: &BytecodeModule,
        layout: &FrameLayout,
    ) -> Option<Self> {
        let (op, lhs, rhs, destination) = match *instruction {
            BytecodeInstruction::BinaryOperands { dst, op, lhs, rhs } => (
                op,
                lhs,
                rhs,
                ScalarDestination::Store(layout.location(dst.index())?.operand.scalar()?),
            ),
            BytecodeInstruction::PredicateBranch {
                op,
                lhs,
                rhs,
                negated,
                then_target,
                else_target,
            } => (
                op,
                lhs,
                rhs,
                ScalarDestination::Branch {
                    negated,
                    then_target,
                    else_target,
                },
            ),
            _ => return None,
        };
        let representation = match lhs {
            ScalarOperand::Slot(slot) => function.metadata.slots[slot.index()],
            ScalarOperand::Constant { ty, .. } => builtin_representation(ty),
        };
        let kernel = binary_kernel(op, representation)?;
        let operand = |operand| -> Option<Result<ScalarSlot, u64>> {
            Some(match operand {
                ScalarOperand::Slot(slot) => Ok(layout.location(slot.index())?.operand.scalar()?),
                ScalarOperand::Constant { constant, ty } => Err(module.constants[constant.index()]
                    .numeric_payload(ty)
                    .expect("verified numeric constant")),
            })
        };
        let operands = match (operand(lhs)?, operand(rhs)?) {
            (Ok(lhs), Ok(rhs)) => PreparedOperands::Slots(lhs, rhs),
            (Ok(slot), Err(constant)) => PreparedOperands::SlotConstant {
                slot,
                constant,
                constant_left: false,
            },
            (Err(constant), Ok(slot)) => PreparedOperands::SlotConstant {
                slot,
                constant,
                constant_left: true,
            },
            (Err(lhs), Err(rhs)) => PreparedOperands::Constants(lhs, rhs),
        };
        Some(Self {
            operands,
            kernel,
            destination,
        })
    }
}
