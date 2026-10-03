use crate::representation::host_representation;
mod intrinsics;

use kagari_common::host_interface::HostFunctionDeclaration;
use {
    crate::{
        native_import::NativeImport,
        operations::{BinaryOp, UnaryOp},
        standard::RuntimePrimitive,
    },
    kagari_abi::representation::ValueType,
};

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
) -> Result<(), ContractError> {
    if !import.structurally_valid() || import.generic.is_some() {
        return Err(ContractError::InvalidOperation {
            reason: "invalid provider native import",
        });
    }
    if args.len() != import.signature.params.len() {
        return Err(ContractError::InvalidOperation {
            reason: "native call arity mismatch",
        });
    }
    for (actual, expected) in args.iter().zip(&import.signature.params) {
        expect_type(*actual, expected.representation(), "native call argument")?;
    }

    verify_call_dst(dst, import.signature.result.representation())
}

pub fn verify_host_call(
    dst: Option<ValueType>,
    declaration: &HostFunctionDeclaration,
    args: &[ValueType],
) -> Result<(), ContractError> {
    declaration
        .validate()
        .map_err(|_| ContractError::InvalidOperation {
            reason: "invalid host declaration",
        })?;
    if args.len() != declaration.params.len() {
        return Err(ContractError::InvalidOperation {
            reason: "host call arity mismatch",
        });
    }
    for (arg, param) in args.iter().zip(&declaration.params) {
        expect_type(*arg, host_representation(&param.ty), "host call argument")?;
    }
    verify_call_dst(dst, host_representation(&declaration.return_type))
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
        let (left, right, output) =
            operation
                .contract()
                .ok_or(ContractError::InvalidOperation {
                    reason: "invalid numeric contract",
                })?;
        let right = right.ok_or(ContractError::InvalidOperation {
            reason: "numeric binary requires rhs",
        })?;
        expect_type(lhs, left.representation(), "numeric lhs")?;
        expect_type(rhs, right.representation(), "numeric rhs")?;
        return Ok(output.representation());
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
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge if numeric(lhs) => {
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
