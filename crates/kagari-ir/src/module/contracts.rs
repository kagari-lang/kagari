use super::{BinaryOp, StandardIntrinsic, UnaryOp, ValueType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    TypeMismatch {
        context: &'static str,
        expected: ValueType,
        found: ValueType,
    },
    Intrinsic {
        intrinsic: StandardIntrinsic,
        reason: &'static str,
    },
    InvalidOperation {
        reason: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeHelperKind {
    TypeOf,
    GetField,
    SetField,
    SetIndex,
}

pub(crate) fn verify_runtime_helper_call(
    dst: Option<ValueType>,
    helper: RuntimeHelperKind,
    args: &[ValueType],
) -> Result<(), ContractError> {
    use RuntimeHelperKind::*;
    let arity = match helper {
        TypeOf | GetField => 1,
        SetField => 2,
        SetIndex => 3,
    };
    if args.len() != arity {
        return Err(ContractError::InvalidOperation {
            reason: "runtime helper arity mismatch",
        });
    }
    match helper {
        TypeOf => verify_call_dst(dst, ValueType::Str),
        GetField => {
            expect_type(args[0], ValueType::HeapObject, "reflection field base")?;
            if dst.is_none() {
                return Err(ContractError::InvalidOperation {
                    reason: "reflection field read needs a destination",
                });
            }
            Ok(())
        }
        SetField | SetIndex => {
            expect_type(args[0], ValueType::HeapObject, "reflection write base")?;
            let value = if helper == SetIndex {
                if !matches!(args[1], ValueType::I32 | ValueType::I64) {
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

pub(crate) fn verify_host_call(
    dst: Option<ValueType>,
    declaration: &kagari_common::host_interface::HostFunctionDeclaration,
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
        expect_type(
            *arg,
            ValueType::from_host_type(&param.ty),
            "host call argument",
        )?;
    }
    verify_call_dst(dst, ValueType::from_host_type(&declaration.return_type))
}

pub(crate) fn expect_type(
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

pub(crate) fn unary_result(op: UnaryOp, operand: ValueType) -> Result<ValueType, ContractError> {
    match op {
        UnaryOp::Neg if numeric(operand) => Ok(operand),
        UnaryOp::Not if operand == ValueType::Bool => Ok(ValueType::Bool),
        _ => Err(ContractError::InvalidOperation {
            reason: "invalid unary operand representation",
        }),
    }
}

pub(crate) fn binary_result(
    op: BinaryOp,
    lhs: ValueType,
    rhs: ValueType,
) -> Result<ValueType, ContractError> {
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
        ValueType::I32 | ValueType::I64 | ValueType::F32 | ValueType::F64
    )
}
pub(crate) fn verify_intrinsic(
    dst: Option<ValueType>,
    intrinsic: StandardIntrinsic,
    args: &[ValueType],
) -> Result<(), ContractError> {
    use StandardIntrinsic::*;

    let arity = match intrinsic {
        ValueEq => 2,
        ValueHash | ValueDebug | ValueDisplay => 1,
        _ => {
            kagari_hir::builtin::surface::standard_function_by_intrinsic(intrinsic)
                .ok_or(ContractError::Intrinsic {
                    intrinsic,
                    reason: "missing standard declaration",
                })?
                .arity
        }
    };
    if args.len() != arity {
        return Err(ContractError::Intrinsic {
            intrinsic,
            reason: "arity mismatch",
        });
    }

    match intrinsic {
        ValueEq => {
            if args[0] != args[1] {
                return Err(ContractError::Intrinsic {
                    intrinsic,
                    reason: "equality operands disagree",
                });
            }
            verify_call_dst(dst, ValueType::Bool)?;
        }
        ValueHash => {
            expect_hash_key_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, ValueType::I64)?;
        }
        ValueDebug | ValueDisplay => {
            verify_call_dst(dst, ValueType::Str)?;
        }
        ArrayLen => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::I64)?;
        }
        ArrayIsEmpty => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::Bool)?;
        }
        ArrayGet | ArrayPop | ArrayRemove => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            if matches!(intrinsic, ArrayGet | ArrayRemove) {
                expect_arg_ty(args, 1, ValueType::I64, "standard intrinsic index")?;
            }
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        ArrayPush | ArrayInsert => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            if intrinsic == ArrayInsert {
                expect_arg_ty(args, 1, ValueType::I64, "standard intrinsic index")?;
            }
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        ArrayClear => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        MapNew | SetNew => {
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        MapLen | SetLen | IterLen => {
            expect_iterable_or_heap_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, ValueType::I64)?;
        }
        MapIsEmpty | SetIsEmpty | IterIsEmpty => {
            expect_iterable_or_heap_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, ValueType::Bool)?;
        }
        MapContainsKey | MapGet | MapInsert | MapRemove => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            expect_hash_key_arg(args, 1, intrinsic)?;
            let return_ty = match intrinsic {
                MapContainsKey => ValueType::Bool,
                MapGet | MapRemove => ValueType::HeapObject,
                MapInsert => ValueType::HeapObject,
                _ => unreachable!(),
            };
            verify_call_dst(dst, return_ty)?;
        }
        MapClear | MapKeys | MapValues | MapEntries | SetClear | SetToArray => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        SetContains | SetInsert | SetRemove => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            expect_hash_key_arg(args, 1, intrinsic)?;
            let return_ty = match intrinsic {
                SetContains | SetRemove => ValueType::Bool,
                SetInsert => ValueType::HeapObject,
                _ => unreachable!(),
            };
            verify_call_dst(dst, return_ty)?;
        }
        SetUnion | SetIntersection | SetDifference => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            expect_arg_ty(
                args,
                1,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StringLenBytes | StringLenChars => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::I64)?;
        }
        StringIsEmpty | StringContains | StringStartsWith | StringEndsWith => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            if intrinsic != StringIsEmpty {
                expect_arg_ty(args, 1, ValueType::Str, "standard intrinsic argument")?;
            }
            verify_call_dst(dst, ValueType::Bool)?;
        }
        StringConcat => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            expect_arg_ty(args, 1, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Str)?;
        }
        StringSlice => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            expect_arg_ty(args, 1, ValueType::I64, "standard intrinsic index")?;
            expect_arg_ty(args, 2, ValueType::I64, "standard intrinsic index")?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        OptionIsSome | OptionIsNone | ResultIsOk | ResultIsErr => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::Bool)?;
        }
        OptionUnwrapOr | ResultUnwrapOr => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            let fallback_ty = args[1];
            verify_call_dst(dst, fallback_ty)?;
        }
        OptionOkOr | OptionOkOrElse | OptionMap | OptionAndThen | ResultMap | ResultMapErr
        | ResultAndThen => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            let _ = args[1];
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        IterGet => {
            expect_iterable_or_heap_arg(args, 0, intrinsic)?;
            expect_arg_ty(args, 1, ValueType::I64, "standard intrinsic index")?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        IterToArray => {
            expect_iterable_or_heap_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        IterForEach => {
            expect_iterable_or_heap_arg(args, 0, intrinsic)?;
            let _ = args[1];
            verify_call_dst(dst, ValueType::Unit)?;
        }
        MathMin | MathMax => {
            let lhs = expect_numeric_arg(args, 0, intrinsic)?;
            let rhs = args[1];
            if rhs != lhs {
                return Err(ContractError::TypeMismatch {
                    context: "standard intrinsic numeric argument",
                    expected: lhs,
                    found: rhs,
                });
            }
            verify_call_dst(dst, lhs)?;
        }
        MathClamp => {
            let value = expect_numeric_arg(args, 0, intrinsic)?;
            for arg in &args[1..] {
                expect_type(*arg, value, "standard intrinsic numeric argument")?;
            }
            verify_call_dst(dst, value)?;
        }
        MathAbs => {
            let value = expect_numeric_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, value)?;
        }
        MathFloor | MathCeil | MathRound | MathSqrt | MathSin | MathCos | MathTan => {
            expect_arg_ty(args, 0, ValueType::F64, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::F64)?;
        }
        DebugPrint | DebugPanic => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        DebugAssert => {
            expect_arg_ty(args, 0, ValueType::Bool, "standard intrinsic argument")?;
            expect_arg_ty(args, 1, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        DebugAssertEq => {
            let lhs = args[0];
            let rhs = args[1];
            if lhs != rhs {
                return Err(ContractError::TypeMismatch {
                    context: "standard intrinsic comparable argument",
                    expected: lhs,
                    found: rhs,
                });
            }
            expect_arg_ty(args, 2, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
    }
    Ok(())
}

pub(crate) fn verify_call_dst(
    dst: Option<ValueType>,
    return_type: ValueType,
) -> Result<(), ContractError> {
    match (dst, return_type) {
        (None, ValueType::Unit) => Ok(()),
        (Some(dst), ty) => expect_type(dst, ty, "call dst"),
        (None, ty) => Err(ContractError::TypeMismatch {
            context: "call dst",
            expected: ty,
            found: ValueType::Unit,
        }),
    }
}

fn expect_arg_ty(
    args: &[ValueType],
    index: usize,
    expected: ValueType,
    context: &'static str,
) -> Result<(), ContractError> {
    expect_type(args[index], expected, context)
}

fn expect_hash_key_arg(
    args: &[ValueType],
    index: usize,
    intrinsic: StandardIntrinsic,
) -> Result<(), ContractError> {
    let found = args[index];
    if matches!(
        found,
        ValueType::Unit
            | ValueType::Bool
            | ValueType::I32
            | ValueType::I64
            | ValueType::Str
            | ValueType::HeapObject
    ) {
        Ok(())
    } else {
        Err(ContractError::Intrinsic {
            intrinsic,
            reason: "hash-key representation does not support Eq + Hash",
        })
    }
}

fn expect_iterable_or_heap_arg(
    args: &[ValueType],
    index: usize,
    intrinsic: StandardIntrinsic,
) -> Result<(), ContractError> {
    let found = args[index];
    if matches!(found, ValueType::HeapObject | ValueType::Str) {
        Ok(())
    } else {
        Err(ContractError::Intrinsic {
            intrinsic,
            reason: "iterable argument must be a heap object or String",
        })
    }
}

fn expect_numeric_arg(
    args: &[ValueType],
    index: usize,
    intrinsic: StandardIntrinsic,
) -> Result<ValueType, ContractError> {
    let found = args[index];
    if matches!(
        found,
        ValueType::I32 | ValueType::I64 | ValueType::F32 | ValueType::F64
    ) {
        Ok(found)
    } else {
        Err(ContractError::Intrinsic {
            intrinsic,
            reason: "numeric argument must be i32/i64/f32/f64 bytecode value",
        })
    }
}
