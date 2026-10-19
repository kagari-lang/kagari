//! Physical and canonical-constant contracts for selected scalar operands.
use crate::{
    instruction::{BinaryOp, BytecodeInstruction, ScalarOperand},
    module::{BytecodeFunction, BytecodeModule},
    verifier::{
        BytecodeVerificationError as Error, contract_error, expect_register_ty, ir_binary_op,
        register_ty, verify_jump,
    },
};
use kagari_abi::representation::ValueType;
use kagari_contract::{
    contracts, operations::object_identity_type, representation::builtin_representation,
};

pub(super) fn operand_type(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    operand: ScalarOperand,
) -> Result<ValueType, Error> {
    match operand {
        ScalarOperand::Slot(slot) => register_ty(function, slot),
        ScalarOperand::Constant { constant, ty } => {
            if module
                .constants
                .get(constant.index())
                .and_then(|value| value.numeric_payload(ty))
                .is_none()
            {
                return Err(Error::InvalidOperation {
                    function: function.id,
                    reason: "invalid scalar constant operand",
                });
            }
            Ok(builtin_representation(ty))
        }
    }
}

pub(super) fn verify_identity(
    function: &BytecodeFunction,
    op: BinaryOp,
    lhs: ScalarOperand,
    rhs: ScalarOperand,
) -> Result<(), Error> {
    if matches!(op, BinaryOp::IdentityEq | BinaryOp::IdentityNotEq)
        && [lhs, rhs].iter().any(|operand| match operand {
            ScalarOperand::Slot(slot) => !function
                .metadata
                .semantic
                .slots
                .get(&slot.index())
                .is_some_and(object_identity_type),
            ScalarOperand::Constant { .. } => true,
        })
    {
        return Err(Error::InvalidOperation {
            function: function.id,
            reason: "identity requires statically known object types",
        });
    }
    Ok(())
}

pub(super) fn verify(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    instruction: &BytecodeInstruction,
) -> Result<(), Error> {
    let (op, lhs, rhs, branch) = match *instruction {
        BytecodeInstruction::BinaryOperands { op, lhs, rhs, .. } => (op, lhs, rhs, false),
        BytecodeInstruction::PredicateBranch { op, lhs, rhs, .. } => (op, lhs, rhs, true),
        _ => unreachable!("operand-form verifier"),
    };
    let left = operand_type(module, function, lhs)?;
    let right = operand_type(module, function, rhs)?;
    let result = contracts::binary_result(ir_binary_op(op), left, right)
        .map_err(|error| contract_error(function, error))?;
    verify_identity(function, op, lhs, rhs)?;
    if branch {
        if !matches!(
            op,
            BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::IdentityEq
                | BinaryOp::IdentityNotEq
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
        ) || result != ValueType::Bool
        {
            return Err(Error::InvalidOperation {
                function: function.id,
                reason: "invalid branch predicate",
            });
        }
        if let BytecodeInstruction::PredicateBranch {
            then_target,
            else_target,
            ..
        } = instruction
        {
            verify_jump(function, *then_target)?;
            verify_jump(function, *else_target)?;
        }
    } else {
        if !matches!(
            left,
            ValueType::I32 | ValueType::I64 | ValueType::U64 | ValueType::F32 | ValueType::F64
        ) {
            return Err(Error::InvalidOperation {
                function: function.id,
                reason: "non-numeric scalar operand form",
            });
        }
        if let BytecodeInstruction::BinaryOperands { dst, .. } = instruction {
            expect_register_ty(function, *dst, result, "scalar operand destination")?;
        }
    }
    Ok(())
}
