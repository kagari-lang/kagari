use crate::{
    numeric::{self, method::IntegerMethodContract},
    operations::{BinaryOp, UnaryOp},
    representation::ValueType,
    standard::StandardIntrinsic,
};
use kagari_common::host_interface::HostFunctionDeclaration;

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
        expect_type(
            *arg,
            ValueType::from_host_type(&param.ty),
            "host call argument",
        )?;
    }
    verify_call_dst(dst, ValueType::from_host_type(&declaration.return_type))
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
pub fn verify_intrinsic(
    dst: Option<ValueType>,
    intrinsic: StandardIntrinsic,
    args: &[ValueType],
) -> Result<(), ContractError> {
    let arity = intrinsic.operand_count();
    if args.len() != arity {
        return Err(ContractError::Intrinsic {
            intrinsic,
            reason: "arity mismatch",
        });
    }

    match intrinsic {
        StandardIntrinsic::StringParse => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "parse requires static lowering",
            });
        }
        StandardIntrinsic::ParseNumber(ty) | StandardIntrinsic::ParseRadix(ty) => {
            if !matches!(intrinsic, StandardIntrinsic::ParseNumber(_))
                && ty.integer_layout().is_none()
                || numeric::parsing_error(ty).is_none()
            {
                return Err(ContractError::Intrinsic {
                    intrinsic,
                    reason: "invalid parser type",
                });
            }
            expect_arg_ty(args, 0, ValueType::Str, "parse input")?;
            if matches!(intrinsic, StandardIntrinsic::ParseRadix(_)) {
                expect_arg_ty(args, 1, ValueType::I64, "parse radix")?;
            }
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::Integer(method, receiver) => {
            let contract =
                IntegerMethodContract::new(method, receiver).ok_or(ContractError::Intrinsic {
                    intrinsic,
                    reason: "invalid numeric binding",
                })?;
            for (index, parameter) in contract.parameters().into_iter().enumerate() {
                expect_arg_ty(
                    args,
                    index,
                    ValueType::from_builtin_type(parameter),
                    "numeric parameter",
                )?;
            }
            verify_call_dst(dst, contract.result().representation())?;
        }
        StandardIntrinsic::ArrayRetain
        | StandardIntrinsic::MapRetain
        | StandardIntrinsic::SetRetain
        | StandardIntrinsic::ArraySort
        | StandardIntrinsic::ArraySortBy
        | StandardIntrinsic::ArraySortByKey
        | StandardIntrinsic::ArrayDedup
        | StandardIntrinsic::MapGetOrInsertWith
        | StandardIntrinsic::MapUpdate => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "map update requires static lowering",
            });
        }
        StandardIntrinsic::ArrayRemoveRangePrepare => {
            for index in 0..3 {
                expect_arg_ty(
                    args,
                    index,
                    ValueType::HeapObject,
                    "array range preparation",
                )?;
            }
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::ArrayRemoveRange => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "range removal requires static lowering",
            });
        }
        StandardIntrinsic::ArrayReplaceStorage | StandardIntrinsic::CollectionRetainStorage => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "collection commit target")?;
            expect_arg_ty(args, 1, ValueType::HeapObject, "prepared collection")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::KeyLookupBegin
        | StandardIntrinsic::CollectionMutationBegin
        | StandardIntrinsic::CollectionMutationEnd
        | StandardIntrinsic::IterResume => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "key lookup collection")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::KeyCandidates
        | StandardIntrinsic::KeyMapGet
        | StandardIntrinsic::KeyMapInsert
        | StandardIntrinsic::KeyMapRemove
        | StandardIntrinsic::KeySetContains
        | StandardIntrinsic::KeySetInsert
        | StandardIntrinsic::KeySetRemove => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "key lookup collection")?;
            expect_arg_ty(args, 1, ValueType::I64, "key hash")?;
            if intrinsic != StandardIntrinsic::KeyCandidates {
                expect_arg_ty(args, 2, ValueType::I64, "key token")?;
            }
            verify_call_dst(
                dst,
                if matches!(
                    intrinsic,
                    StandardIntrinsic::KeySetContains | StandardIntrinsic::KeySetRemove
                ) {
                    ValueType::Bool
                } else {
                    ValueType::HeapObject
                },
            )?;
        }
        StandardIntrinsic::ValuePartialCmp | StandardIntrinsic::ValueCmp => {
            if args[0] != args[1] {
                return Err(ContractError::Intrinsic {
                    intrinsic,
                    reason: "comparison operands differ",
                });
            }
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::ValueEq => {
            if args[0] != args[1] {
                return Err(ContractError::Intrinsic {
                    intrinsic,
                    reason: "equality operands disagree",
                });
            }
            verify_call_dst(dst, ValueType::Bool)?;
        }
        StandardIntrinsic::ValueHash => {
            expect_hash_key_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, ValueType::I64)?;
        }
        StandardIntrinsic::ValueDebug | StandardIntrinsic::ValueDisplay => {
            verify_call_dst(dst, ValueType::Str)?;
        }
        StandardIntrinsic::ArrayLen => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::U64)?;
        }
        StandardIntrinsic::ArrayIsEmpty => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::Bool)?;
        }
        StandardIntrinsic::ArrayGet
        | StandardIntrinsic::ArrayPop
        | StandardIntrinsic::ArrayRemove
        | StandardIntrinsic::ArraySwapRemove => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            if matches!(
                intrinsic,
                StandardIntrinsic::ArrayGet
                    | StandardIntrinsic::ArrayRemove
                    | StandardIntrinsic::ArraySwapRemove
            ) {
                expect_arg_ty(args, 1, ValueType::U64, "standard intrinsic index")?;
            }
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::ArrayWithCapacity
        | StandardIntrinsic::MapWithCapacity
        | StandardIntrinsic::SetWithCapacity => {
            expect_arg_ty(args, 0, ValueType::U64, "initial capacity")?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::ArrayCapacity
        | StandardIntrinsic::MapCapacity
        | StandardIntrinsic::SetCapacity => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "collection receiver")?;
            verify_call_dst(dst, ValueType::U64)?;
        }
        StandardIntrinsic::ArrayReserve
        | StandardIntrinsic::MapReserve
        | StandardIntrinsic::SetReserve => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "collection receiver")?;
            expect_arg_ty(args, 1, ValueType::U64, "additional capacity")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::ArraySwap
        | StandardIntrinsic::ArrayReverse
        | StandardIntrinsic::ArrayTruncate
        | StandardIntrinsic::ArrayExtendStorage => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "array mutation receiver")?;
            if intrinsic == StandardIntrinsic::ArrayExtendStorage {
                expect_arg_ty(args, 1, ValueType::HeapObject, "array extension storage")?;
            }
            if matches!(
                intrinsic,
                StandardIntrinsic::ArraySwap | StandardIntrinsic::ArrayTruncate
            ) {
                expect_arg_ty(args, 1, ValueType::U64, "array mutation index")?;
            }
            if intrinsic == StandardIntrinsic::ArraySwap {
                expect_arg_ty(args, 2, ValueType::U64, "array mutation index")?;
            }
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::ArrayPush | StandardIntrinsic::ArrayInsert => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            if intrinsic == StandardIntrinsic::ArrayInsert {
                expect_arg_ty(args, 1, ValueType::U64, "standard intrinsic index")?;
            }
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::ArrayExtend
        | StandardIntrinsic::StringBytes
        | StandardIntrinsic::StringCharIndices
        | StandardIntrinsic::StringSplit
        | StandardIntrinsic::StringSplitN
        | StandardIntrinsic::StringSplitWhitespace
        | StandardIntrinsic::StringLines
        | StandardIntrinsic::ArrayCopyFrom
        | StandardIntrinsic::ArrayCopyWithin
        | StandardIntrinsic::ArrayListFromFn
        | StandardIntrinsic::MapKeys
        | StandardIntrinsic::MapValues
        | StandardIntrinsic::MapEntries => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "callback and protocol calls require static lowering",
            });
        }
        StandardIntrinsic::ArrayCopyWithinBounds => {
            for index in 0..3 {
                expect_arg_ty(args, index, ValueType::HeapObject, "array range operand")?;
            }
            expect_arg_ty(args, 3, ValueType::U64, "array copy destination")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::ArrayFill | StandardIntrinsic::ArrayCopyFromStorage => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "array target")?;
            if intrinsic == StandardIntrinsic::ArrayCopyFromStorage {
                expect_arg_ty(args, 1, ValueType::HeapObject, "array source")?;
            }
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::ArrayClear => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::LinkedHashMapNew
        | StandardIntrinsic::LinkedHashSetNew
        | StandardIntrinsic::ArrayListNew => {
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::ArrayListFrom
        | StandardIntrinsic::LinkedHashMapFrom
        | StandardIntrinsic::LinkedHashSetFrom => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "collection factories require checked construction lowering",
            });
        }
        StandardIntrinsic::MapLen | StandardIntrinsic::SetLen => {
            expect_iterable_or_heap_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, ValueType::U64)?;
        }
        StandardIntrinsic::MapIsEmpty | StandardIntrinsic::SetIsEmpty => {
            expect_iterable_or_heap_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, ValueType::Bool)?;
        }
        StandardIntrinsic::MapContainsKey
        | StandardIntrinsic::MapGet
        | StandardIntrinsic::MapInsert
        | StandardIntrinsic::MapRemove => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            expect_hash_key_arg(args, 1, intrinsic)?;
            let return_ty = match intrinsic {
                StandardIntrinsic::MapContainsKey => ValueType::Bool,
                StandardIntrinsic::MapGet | StandardIntrinsic::MapRemove => ValueType::HeapObject,
                StandardIntrinsic::MapInsert => ValueType::HeapObject,
                _ => unreachable!(),
            };
            verify_call_dst(dst, return_ty)?;
        }
        StandardIntrinsic::MapClear
        | StandardIntrinsic::MapKeysStorage
        | StandardIntrinsic::MapValuesStorage
        | StandardIntrinsic::MapEntriesStorage
        | StandardIntrinsic::SetClear
        | StandardIntrinsic::SetToArray => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::SetContains
        | StandardIntrinsic::SetInsert
        | StandardIntrinsic::SetRemove => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            expect_hash_key_arg(args, 1, intrinsic)?;
            let return_ty = match intrinsic {
                StandardIntrinsic::SetContains | StandardIntrinsic::SetRemove => ValueType::Bool,
                StandardIntrinsic::SetInsert => ValueType::HeapObject,
                _ => unreachable!(),
            };
            verify_call_dst(dst, return_ty)?;
        }

        StandardIntrinsic::StringLenBytes | StandardIntrinsic::StringLenChars => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::U64)?;
        }
        StandardIntrinsic::StringIsEmpty
        | StandardIntrinsic::StringContains
        | StandardIntrinsic::StringStartsWith
        | StandardIntrinsic::StringEndsWith => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            if intrinsic != StandardIntrinsic::StringIsEmpty {
                expect_arg_ty(args, 1, ValueType::Str, "standard intrinsic argument")?;
            }
            verify_call_dst(dst, ValueType::Bool)?;
        }
        StandardIntrinsic::ArrayJoin => {
            expect_arg_ty(args, 0, ValueType::HeapObject, "string array")?;
            expect_arg_ty(args, 1, ValueType::Str, "join separator")?;
            verify_call_dst(dst, ValueType::Str)?;
        }
        StandardIntrinsic::StringReplace | StandardIntrinsic::StringReplaceN => {
            for i in 0..3 {
                expect_arg_ty(args, i, ValueType::Str, "string replacement argument")?;
            }
            if intrinsic == StandardIntrinsic::StringReplaceN {
                expect_arg_ty(args, 3, ValueType::U64, "replacement limit")?;
            }
            verify_call_dst(dst, ValueType::Str)?;
        }
        StandardIntrinsic::StringRepeat | StandardIntrinsic::StringIsCharBoundary => {
            expect_arg_ty(args, 0, ValueType::Str, "string receiver")?;
            expect_arg_ty(args, 1, ValueType::U64, "string count/index")?;
            verify_call_dst(
                dst,
                if intrinsic == StandardIntrinsic::StringRepeat {
                    ValueType::Str
                } else {
                    ValueType::Bool
                },
            )?;
        }
        StandardIntrinsic::StringIsAscii | StandardIntrinsic::StringEqIgnoreAsciiCase => {
            expect_arg_ty(args, 0, ValueType::Str, "string receiver")?;
            if intrinsic == StandardIntrinsic::StringEqIgnoreAsciiCase {
                expect_arg_ty(args, 1, ValueType::Str, "string comparison")?;
            }
            verify_call_dst(dst, ValueType::Bool)?;
        }
        StandardIntrinsic::StringToAsciiLowercase
        | StandardIntrinsic::StringToAsciiUppercase
        | StandardIntrinsic::StringToLowercase
        | StandardIntrinsic::StringToUppercase
        | StandardIntrinsic::StringTrim
        | StandardIntrinsic::StringTrimStart
        | StandardIntrinsic::StringTrimEnd => {
            expect_arg_ty(args, 0, ValueType::Str, "string receiver")?;
            verify_call_dst(dst, ValueType::Str)?;
        }
        StandardIntrinsic::StringFind
        | StandardIntrinsic::StringRfind
        | StandardIntrinsic::StringStripPrefix
        | StandardIntrinsic::StringStripSuffix
        | StandardIntrinsic::StringSplitOnce
        | StandardIntrinsic::StringRsplitOnce => {
            expect_arg_ty(args, 0, ValueType::Str, "string receiver")?;
            expect_arg_ty(args, 1, ValueType::Str, "string pattern")?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::StringConcat => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            expect_arg_ty(args, 1, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Str)?;
        }
        StandardIntrinsic::StringSlice => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            expect_arg_ty(args, 1, ValueType::U64, "standard intrinsic index")?;
            expect_arg_ty(args, 2, ValueType::U64, "standard intrinsic index")?;
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::OptionUnwrapOrElse
        | StandardIntrinsic::OptionOrElse
        | StandardIntrinsic::OptionMapOr
        | StandardIntrinsic::OptionMapOrElse
        | StandardIntrinsic::OptionFilter
        | StandardIntrinsic::OptionIsSomeAnd
        | StandardIntrinsic::OptionZip
        | StandardIntrinsic::OptionFlatten
        | StandardIntrinsic::OptionTranspose
        | StandardIntrinsic::ResultUnwrapOrElse
        | StandardIntrinsic::ResultOrElse
        | StandardIntrinsic::ResultMapOr
        | StandardIntrinsic::ResultMapOrElse
        | StandardIntrinsic::ResultOk
        | StandardIntrinsic::ResultErr
        | StandardIntrinsic::ResultIsOkAnd
        | StandardIntrinsic::ResultIsErrAnd
        | StandardIntrinsic::ResultFlatten
        | StandardIntrinsic::ResultTranspose => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "enum combinator requires lowering",
            });
        }
        StandardIntrinsic::OptionIsSome
        | StandardIntrinsic::OptionIsNone
        | StandardIntrinsic::ResultIsOk
        | StandardIntrinsic::ResultIsErr => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            verify_call_dst(dst, ValueType::Bool)?;
        }
        StandardIntrinsic::OptionUnwrapOr | StandardIntrinsic::ResultUnwrapOr => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            let fallback_ty = args[1];
            verify_call_dst(dst, fallback_ty)?;
        }
        StandardIntrinsic::OptionOkOr
        | StandardIntrinsic::OptionOkOrElse
        | StandardIntrinsic::OptionMap
        | StandardIntrinsic::OptionAndThen
        | StandardIntrinsic::ResultMap
        | StandardIntrinsic::ResultMapErr
        | StandardIntrinsic::ResultAndThen => {
            expect_arg_ty(
                args,
                0,
                ValueType::HeapObject,
                "standard intrinsic argument",
            )?;
            let _ = args[1];
            verify_call_dst(dst, ValueType::HeapObject)?;
        }
        StandardIntrinsic::MathMin | StandardIntrinsic::MathMax => {
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
        StandardIntrinsic::MathClamp => {
            let value = expect_numeric_arg(args, 0, intrinsic)?;
            for arg in &args[1..] {
                expect_type(*arg, value, "standard intrinsic numeric argument")?;
            }
            verify_call_dst(dst, value)?;
        }
        StandardIntrinsic::MathAbs => {
            let value = expect_numeric_arg(args, 0, intrinsic)?;
            verify_call_dst(dst, value)?;
        }
        StandardIntrinsic::MathFloor
        | StandardIntrinsic::MathCeil
        | StandardIntrinsic::MathRound
        | StandardIntrinsic::MathSqrt
        | StandardIntrinsic::MathSin
        | StandardIntrinsic::MathCos
        | StandardIntrinsic::MathTan => {
            expect_arg_ty(args, 0, ValueType::F64, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::F64)?;
        }
        StandardIntrinsic::DebugPrint => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::DebugPanic => {
            expect_arg_ty(args, 0, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Never)?;
        }
        StandardIntrinsic::DebugAssert => {
            expect_arg_ty(args, 0, ValueType::Bool, "standard intrinsic argument")?;
            expect_arg_ty(args, 1, ValueType::Str, "standard intrinsic argument")?;
            verify_call_dst(dst, ValueType::Unit)?;
        }
        StandardIntrinsic::DebugAssertEq => {
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
            | ValueType::U64
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
        ValueType::I32 | ValueType::I64 | ValueType::U64 | ValueType::F32 | ValueType::F64
    ) {
        Ok(found)
    } else {
        Err(ContractError::Intrinsic {
            intrinsic,
            reason: "numeric argument must be i32/i64/f32/f64 bytecode value",
        })
    }
}
