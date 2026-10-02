use crate::{
    contracts::{ContractError, verify_call_dst},
    representation::ValueType,
    standard::RuntimePrimitive as Intrinsic,
};
pub(super) fn verify(
    dst: Option<ValueType>,
    intrinsic: Intrinsic,
    args: &[ValueType],
) -> Result<(), ContractError> {
    let result = match (intrinsic, args) {
        (Intrinsic::ValuePartialCmp | Intrinsic::ValueCmp, [left, right]) if left == right => {
            ValueType::HeapObject
        }
        (Intrinsic::ValueEq, [left, right]) if left == right => ValueType::Bool,
        (
            Intrinsic::ValueHash,
            [
                ValueType::Unit
                | ValueType::Bool
                | ValueType::I32
                | ValueType::I64
                | ValueType::U64
                | ValueType::Str
                | ValueType::HeapObject,
            ],
        ) => ValueType::I64,
        (Intrinsic::ValueDebug | Intrinsic::ValueDisplay, [_]) => ValueType::Str,
        (Intrinsic::StringPartsJoin, [ValueType::HeapObject, ValueType::Str]) => ValueType::Str,
        (Intrinsic::Assert, [ValueType::Bool, ValueType::Str]) => ValueType::Unit,
        _ => {
            return Err(ContractError::Intrinsic {
                intrinsic,
                reason: "invalid language primitive operands",
            });
        }
    };
    verify_call_dst(dst, result)
}
