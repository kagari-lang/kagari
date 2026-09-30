//! Physical storage operations. Slice patterns validate the operands consumed by
//! each implementation; public callable arity comes from its carried signature.
use crate::{
    contracts::{ContractError, expect_type, verify_call_dst},
    numeric::{self, method::IntegerMethodContract},
    representation::ValueType,
    standard::StandardIntrinsic as Intrinsic,
};

pub(super) fn verify(
    dst: Option<ValueType>,
    intrinsic: Intrinsic,
    args: &[ValueType],
) -> Result<(), ContractError> {
    let result = match (intrinsic, args) {
        (Intrinsic::ParseNumber(scalar), [ValueType::Str])
            if numeric::parsing_error(scalar).is_some() =>
        {
            ValueType::HeapObject
        }
        (Intrinsic::ParseRadix(scalar), [ValueType::Str, ValueType::I64])
            if scalar.integer_layout().is_some() =>
        {
            ValueType::HeapObject
        }
        (Intrinsic::Integer(method, scalar), [receiver, rhs]) => {
            let contract =
                IntegerMethodContract::new(method, scalar).ok_or(ContractError::Intrinsic {
                    intrinsic,
                    reason: "invalid numeric binding",
                })?;
            let [left, right] = contract.parameters();
            expect_type(
                *receiver,
                ValueType::from_builtin_type(left),
                "numeric parameter",
            )?;
            expect_type(
                *rhs,
                ValueType::from_builtin_type(right),
                "numeric parameter",
            )?;
            contract.result().representation()
        }
        (
            Intrinsic::ArrayRemoveRangePrepare,
            [
                ValueType::HeapObject,
                ValueType::HeapObject,
                ValueType::HeapObject,
            ],
        ) => ValueType::HeapObject,
        (
            Intrinsic::ArrayReplaceStorage
            | Intrinsic::CollectionRetainStorage
            | Intrinsic::ArrayCopyFromStorage
            | Intrinsic::ArrayExtendStorage,
            [ValueType::HeapObject, ValueType::HeapObject],
        ) => ValueType::Unit,
        (
            Intrinsic::KeyLookupBegin
            | Intrinsic::CollectionMutationBegin
            | Intrinsic::CollectionMutationEnd
            | Intrinsic::IterResume,
            [ValueType::HeapObject],
        ) => ValueType::Unit,
        (Intrinsic::KeyCandidates, [ValueType::HeapObject, ValueType::I64]) => {
            ValueType::HeapObject
        }
        (
            Intrinsic::KeyMapGet | Intrinsic::KeyMapRemove,
            [ValueType::HeapObject, ValueType::I64, ValueType::I64],
        ) => ValueType::HeapObject,
        (
            Intrinsic::KeyMapInsert,
            [ValueType::HeapObject, ValueType::I64, ValueType::I64, _, _],
        ) => ValueType::HeapObject,
        (
            Intrinsic::KeySetContains | Intrinsic::KeySetRemove,
            [ValueType::HeapObject, ValueType::I64, ValueType::I64],
        ) => ValueType::Bool,
        (Intrinsic::KeySetInsert, [ValueType::HeapObject, ValueType::I64, ValueType::I64, _]) => {
            ValueType::HeapObject
        }
        (Intrinsic::ValuePartialCmp | Intrinsic::ValueCmp, [left, right]) if left == right => {
            ValueType::HeapObject
        }
        (Intrinsic::ValueEq, [left, right]) if left == right => ValueType::Bool,
        (Intrinsic::ValueHash, [key]) if hash_key(*key) => ValueType::I64,
        (Intrinsic::ValueDebug | Intrinsic::ValueDisplay, [_]) => ValueType::Str,
        (
            Intrinsic::ArrayLen
            | Intrinsic::ArrayCapacity
            | Intrinsic::MapCapacity
            | Intrinsic::SetCapacity,
            [ValueType::HeapObject],
        ) => ValueType::U64,
        (Intrinsic::ArrayIsEmpty, [ValueType::HeapObject]) => ValueType::Bool,
        (
            Intrinsic::ArrayGet | Intrinsic::ArrayRemove | Intrinsic::ArraySwapRemove,
            [ValueType::HeapObject, ValueType::U64],
        ) => ValueType::HeapObject,
        (Intrinsic::ArrayPop | Intrinsic::ArrayClear, [ValueType::HeapObject]) => {
            ValueType::HeapObject
        }
        (
            Intrinsic::ArrayWithCapacity | Intrinsic::MapWithCapacity | Intrinsic::SetWithCapacity,
            [ValueType::U64],
        ) => ValueType::HeapObject,
        (
            Intrinsic::ArrayReserve
            | Intrinsic::MapReserve
            | Intrinsic::SetReserve
            | Intrinsic::ArrayTruncate,
            [ValueType::HeapObject, ValueType::U64],
        ) => ValueType::Unit,
        (Intrinsic::ArraySwap, [ValueType::HeapObject, ValueType::U64, ValueType::U64]) => {
            ValueType::Unit
        }
        (Intrinsic::ArrayReverse, [ValueType::HeapObject]) => ValueType::Unit,
        (Intrinsic::ArrayPush, [ValueType::HeapObject, _])
        | (Intrinsic::ArrayInsert, [ValueType::HeapObject, ValueType::U64, _]) => {
            ValueType::HeapObject
        }
        (Intrinsic::ArrayFill, [ValueType::HeapObject, _]) => ValueType::Unit,
        (
            Intrinsic::ArrayCopyWithinBounds,
            [
                ValueType::HeapObject,
                ValueType::HeapObject,
                ValueType::HeapObject,
                ValueType::U64,
            ],
        ) => ValueType::Unit,
        (
            Intrinsic::LinkedHashMapNew | Intrinsic::LinkedHashSetNew | Intrinsic::ArrayListNew,
            [],
        ) => ValueType::HeapObject,
        (Intrinsic::MapLen | Intrinsic::SetLen, [ValueType::HeapObject | ValueType::Str]) => {
            ValueType::U64
        }
        (
            Intrinsic::MapIsEmpty | Intrinsic::SetIsEmpty,
            [ValueType::HeapObject | ValueType::Str],
        ) => ValueType::Bool,
        (
            Intrinsic::MapContainsKey | Intrinsic::SetContains | Intrinsic::SetRemove,
            [ValueType::HeapObject, key],
        ) if hash_key(*key) => ValueType::Bool,
        (
            Intrinsic::MapGet | Intrinsic::MapRemove | Intrinsic::SetInsert,
            [ValueType::HeapObject, key],
        ) if hash_key(*key) => ValueType::HeapObject,
        (Intrinsic::MapInsert, [ValueType::HeapObject, key, _]) if hash_key(*key) => {
            ValueType::HeapObject
        }
        (
            Intrinsic::MapClear
            | Intrinsic::MapKeysStorage
            | Intrinsic::MapValuesStorage
            | Intrinsic::MapEntriesStorage
            | Intrinsic::SetClear
            | Intrinsic::SetToArray,
            [ValueType::HeapObject],
        ) => ValueType::HeapObject,
        (Intrinsic::StringLenBytes | Intrinsic::StringLenChars, [ValueType::Str]) => ValueType::U64,
        (Intrinsic::StringIsEmpty | Intrinsic::StringIsAscii, [ValueType::Str]) => ValueType::Bool,
        (
            Intrinsic::StringContains
            | Intrinsic::StringStartsWith
            | Intrinsic::StringEndsWith
            | Intrinsic::StringEqIgnoreAsciiCase,
            [ValueType::Str, ValueType::Str],
        ) => ValueType::Bool,
        (Intrinsic::ArrayJoin, [ValueType::HeapObject, ValueType::Str]) => ValueType::Str,
        (Intrinsic::StringReplace, [ValueType::Str, ValueType::Str, ValueType::Str])
        | (
            Intrinsic::StringReplaceN,
            [
                ValueType::Str,
                ValueType::Str,
                ValueType::Str,
                ValueType::U64,
            ],
        ) => ValueType::Str,
        (Intrinsic::StringRepeat, [ValueType::Str, ValueType::U64]) => ValueType::Str,
        (Intrinsic::StringIsCharBoundary, [ValueType::Str, ValueType::U64]) => ValueType::Bool,
        (
            Intrinsic::StringToAsciiLowercase
            | Intrinsic::StringToAsciiUppercase
            | Intrinsic::StringToLowercase
            | Intrinsic::StringToUppercase
            | Intrinsic::StringTrim
            | Intrinsic::StringTrimStart
            | Intrinsic::StringTrimEnd,
            [ValueType::Str],
        ) => ValueType::Str,
        (
            Intrinsic::StringFind
            | Intrinsic::StringRfind
            | Intrinsic::StringStripPrefix
            | Intrinsic::StringStripSuffix
            | Intrinsic::StringSplitOnce
            | Intrinsic::StringRsplitOnce,
            [ValueType::Str, ValueType::Str],
        ) => ValueType::HeapObject,
        (Intrinsic::StringConcat, [ValueType::Str, ValueType::Str]) => ValueType::Str,
        (Intrinsic::StringSlice, [ValueType::Str, ValueType::U64, ValueType::U64]) => {
            ValueType::HeapObject
        }
        (
            Intrinsic::OptionIsSome
            | Intrinsic::OptionIsNone
            | Intrinsic::ResultIsOk
            | Intrinsic::ResultIsErr,
            [ValueType::HeapObject],
        ) => ValueType::Bool,
        (
            Intrinsic::OptionUnwrapOr | Intrinsic::ResultUnwrapOr,
            [ValueType::HeapObject, fallback],
        ) => *fallback,
        (Intrinsic::MathMin | Intrinsic::MathMax, [left, right])
            if numeric(*left) && left == right =>
        {
            *left
        }
        (Intrinsic::MathClamp, [value, min, max])
            if numeric(*value) && value == min && value == max =>
        {
            *value
        }
        (Intrinsic::MathAbs, [value]) if numeric(*value) => *value,
        (
            Intrinsic::MathFloor
            | Intrinsic::MathCeil
            | Intrinsic::MathRound
            | Intrinsic::MathSqrt
            | Intrinsic::MathSin
            | Intrinsic::MathCos
            | Intrinsic::MathTan,
            [ValueType::F64],
        ) => ValueType::F64,
        (Intrinsic::DebugPrint, [ValueType::Str]) => ValueType::Unit,
        (Intrinsic::DebugPanic, [ValueType::Str]) => ValueType::Never,
        (Intrinsic::DebugAssert, [ValueType::Bool, ValueType::Str]) => ValueType::Unit,
        (Intrinsic::DebugAssertEq, [left, right, ValueType::Str]) if left == right => {
            ValueType::Unit
        }
        _ => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "invalid or unsupported native operand shape",
            });
        }
    };
    verify_call_dst(dst, result)
}

fn numeric(ty: ValueType) -> bool {
    matches!(
        ty,
        ValueType::I32 | ValueType::I64 | ValueType::U64 | ValueType::F32 | ValueType::F64
    )
}

fn hash_key(ty: ValueType) -> bool {
    matches!(
        ty,
        ValueType::Unit
            | ValueType::Bool
            | ValueType::I32
            | ValueType::I64
            | ValueType::U64
            | ValueType::Str
            | ValueType::HeapObject
    )
}
