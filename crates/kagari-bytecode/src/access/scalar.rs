//! Full semantic facts for both retained slot and selected constant numeric forms.
use crate::{
    access::{Fact, flows},
    instruction::{BinaryOp, ScalarOperand, Slot},
    module::BytecodeFunction,
    verifier::BytecodeVerificationError as Error,
};
use kagari_abi::representation::ValueType;
use kagari_contract::{numeric::NumericOperation, representation::builtin_representation};
use kagari_types::{scalar::BuiltinType as B, ty::Ty};

fn operand_flows(
    operand: ScalarOperand,
    expected: &Ty,
    get: &impl Fn(Slot) -> Option<Fact>,
) -> bool {
    match operand {
        ScalarOperand::Slot(slot) => get(slot).is_none_or(|fact| flows(&fact, expected)),
        ScalarOperand::Constant { ty, .. } => &Ty::Builtin(ty) == expected,
    }
}

fn builtin(operand: ScalarOperand, get: &impl Fn(Slot) -> Option<Fact>) -> Option<B> {
    match operand {
        ScalarOperand::Constant { ty, .. } => Some(ty),
        ScalarOperand::Slot(slot) => get(slot).and_then(|fact| match fact.ty.as_deref() {
            Some(Ty::Builtin(ty)) => Some(*ty),
            _ => None,
        }),
    }
}

pub(super) fn numeric(
    operation: NumericOperation,
    lhs: ScalarOperand,
    rhs: Option<ScalarOperand>,
    get: &impl Fn(Slot) -> Option<Fact>,
    invalid: &impl Fn() -> Error,
) -> Result<Fact, Error> {
    let signature = operation.contract().ok_or_else(invalid)?;
    if !operand_flows(lhs, &signature.left, get) {
        return Err(invalid());
    }
    if let (Some(rhs), Some(expected)) = (rhs, signature.right.as_ref())
        && !operand_flows(rhs, expected, get)
    {
        return Err(invalid());
    }
    Ok(Fact::typed(signature.output))
}

pub(super) fn binary(
    function: &BytecodeFunction,
    op: BinaryOp,
    lhs: ScalarOperand,
    rhs: ScalarOperand,
    get: &impl Fn(Slot) -> Option<Fact>,
    invalid: &impl Fn() -> Error,
) -> Result<Fact, Error> {
    if let BinaryOp::Numeric(operation) = op {
        return numeric(operation, lhs, Some(rhs), get, invalid);
    }
    if let (Some(left), Some(right)) = (builtin(lhs, get), builtin(rhs, get))
        && left != right
    {
        return Err(invalid());
    }
    let comparison = matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IdentityEq
            | BinaryOp::IdentityNotEq
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
    );
    let physical = match lhs {
        ScalarOperand::Slot(slot) => function.metadata.slots[slot.index()],
        ScalarOperand::Constant { ty, .. } => builtin_representation(ty),
    };
    let result = if comparison {
        B::Bool
    } else {
        match physical {
            ValueType::I32 => B::I32,
            ValueType::I64 => B::I64,
            ValueType::U64 => B::U64,
            ValueType::F32 => B::F32,
            ValueType::F64 => B::F64,
            _ => return Err(invalid()),
        }
    };
    Ok(Fact::typed(Ty::Builtin(result)))
}
