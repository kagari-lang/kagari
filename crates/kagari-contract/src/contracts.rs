use crate::numeric::NumericSignature;
use crate::{
    callable::receiver::accepts_owned,
    native_import::NativeImport,
    operations::{BinaryOp, UnaryOp},
    representation::semantic_representation,
    standard::RuntimePrimitive,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionTable;

mod intrinsics;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    TypeMismatch {
        context: &'static str,
        expected: ValueType,
        found: ValueType,
    },
    Intrinsic {
        intrinsic: RuntimePrimitive,
        reason: &'static str,
    },
    InvalidOperation {
        reason: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHelperKind {
    TypeOf,
    GetField,
    SetField,
    SetIndex,
}

pub fn verify_runtime_helper_call(
    dst: Option<ValueType>,
    helper: RuntimeHelperKind,
    args: &[ValueType],
) -> Result<(), ContractError> {
    let arity = match helper {
        RuntimeHelperKind::TypeOf | RuntimeHelperKind::GetField => 1,
        RuntimeHelperKind::SetField => 2,
        RuntimeHelperKind::SetIndex => 3,
    };
    if args.len() != arity {
        return Err(ContractError::InvalidOperation {
            reason: "runtime helper arity mismatch",
        });
    }
    match helper {
        RuntimeHelperKind::TypeOf => verify_call_dst(dst, ValueType::Str),
        RuntimeHelperKind::GetField => {
            expect_type(args[0], ValueType::HeapObject, "reflection field base")?;
            if dst.is_none() {
                return Err(ContractError::InvalidOperation {
                    reason: "reflection field read needs a destination",
                });
            }
            Ok(())
        }
        RuntimeHelperKind::SetField | RuntimeHelperKind::SetIndex => {
            expect_type(args[0], ValueType::HeapObject, "reflection write base")?;
            let value = if helper == RuntimeHelperKind::SetIndex {
                if !matches!(args[1], ValueType::I32 | ValueType::I64 | ValueType::U64) {
                    return Err(ContractError::InvalidOperation {
                        reason: "reflection index must be an integer",
                    });
                }
                args[2]
            } else {
                args[1]
            };
            if value == ValueType::HostHandle {
                return Err(ContractError::InvalidOperation {
                    reason: "host handles cannot be stored by reflection",
                });
            }
            verify_call_dst(dst, ValueType::HeapObject)
        }
    }
}

/// The signature sets call arity and operand representations. The closed engine
/// guard independently checks its consumed storage and result shape.
pub fn verify_native_call(
    dst: Option<ValueType>,
    import: &NativeImport,
    args: &[ValueType],
    definitions: &DefinitionTable,
) -> Result<(), ContractError> {
    if !import.structurally_valid(definitions) || import.generic.is_some() {
        return Err(ContractError::InvalidOperation {
            reason: "invalid provider native import",
        });
    }
    if args.len() != import.signature.params.len() {
        return Err(ContractError::InvalidOperation {
            reason: "native call arity mismatch",
        });
    }
    for (index, (actual, expected)) in args.iter().zip(&import.signature.params).enumerate() {
        if index == 0 && *actual == ValueType::HostHandle && import.receiver_access.is_some() {
            // Semantic flow verification must additionally prove the same target
            // and sufficient view access. This is storage adaptation, not a cast.
            continue;
        }
        if index == 0 && !accepts_owned(import.receiver_access, expected) {
            return Err(ContractError::InvalidOperation {
                reason: "Rust struct receiver requires an external scoped location",
            });
        }
        expect_type(
            *actual,
            semantic_representation(expected),
            "native call argument",
        )?;
    }

    verify_call_dst(dst, semantic_representation(&import.signature.result))
}

pub fn expect_type(
    found: ValueType,
    expected: ValueType,
    context: &'static str,
) -> Result<(), ContractError> {
    if found == expected {
        Ok(())
    } else {
        Err(ContractError::TypeMismatch {
            context,
            expected,
            found,
        })
    }
}

pub fn unary_result(op: UnaryOp, operand: ValueType) -> Result<ValueType, ContractError> {
    match op {
        UnaryOp::Neg if numeric(operand) && operand != ValueType::U64 => Ok(operand),
        UnaryOp::Not if operand == ValueType::Bool => Ok(ValueType::Bool),
        _ => Err(ContractError::InvalidOperation {
            reason: "invalid unary operand representation",
        }),
    }
}

pub fn binary_result(
    op: BinaryOp,
    lhs: ValueType,
    rhs: ValueType,
) -> Result<ValueType, ContractError> {
    if let BinaryOp::Numeric(operation) = op {
        let NumericSignature {
            left,
            right,
            output,
        } = operation
            .contract()
            .ok_or(ContractError::InvalidOperation {
                reason: "invalid numeric contract",
            })?;
        let right = right.ok_or(ContractError::InvalidOperation {
            reason: "numeric binary requires rhs",
        })?;
        expect_type(lhs, semantic_representation(&left), "numeric lhs")?;
        expect_type(rhs, semantic_representation(&right), "numeric rhs")?;
        return Ok(semantic_representation(&output));
    }
    expect_type(rhs, lhs, "binary rhs")?;
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem
            if numeric(lhs) =>
        {
            Ok(lhs)
        }
        BinaryOp::Eq | BinaryOp::NotEq if lhs != ValueType::HostHandle => Ok(ValueType::Bool),
        BinaryOp::IdentityEq | BinaryOp::IdentityNotEq if lhs == ValueType::HeapObject => {
            Ok(ValueType::Bool)
        }
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge
            if numeric(lhs) || lhs == ValueType::Char =>
        {
            Ok(ValueType::Bool)
        }
        _ => Err(ContractError::InvalidOperation {
            reason: "invalid binary operand representation or unlowered short-circuit operation",
        }),
    }
}

fn numeric(ty: ValueType) -> bool {
    matches!(
        ty,
        ValueType::I32 | ValueType::I64 | ValueType::U64 | ValueType::F32 | ValueType::F64
    )
}

/// Validate the physical operands actually consumed by an engine operation.
/// Ordinary native calls additionally validate their carried semantic signature.
pub fn verify_intrinsic(
    dst: Option<ValueType>,
    intrinsic: RuntimePrimitive,
    args: &[ValueType],
) -> Result<(), ContractError> {
    intrinsics::verify(dst, intrinsic, args)
}

pub fn verify_call_dst(
    dst: Option<ValueType>,
    return_type: ValueType,
) -> Result<(), ContractError> {
    match (dst, return_type) {
        (None, ValueType::Unit | ValueType::Never) => Ok(()),
        (Some(dst), ty) => expect_type(dst, ty, "call dst"),
        (None, ty) => Err(ContractError::TypeMismatch {
            context: "call dst",
            expected: ty,
            found: ValueType::Unit,
        }),
    }
}
