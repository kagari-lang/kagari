//! Semantic propagation for low-level storage instructions. Public callable
//! signatures are carried by native imports; these guards preserve slot types
//! when compiler-generated protocols manipulate containers directly.
use crate::access::{Fact, flows};
use kagari_abi::{
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum},
    types::AbiType,
};

pub(super) fn validate(operation: StandardIntrinsic, facts: &[Fact]) -> Result<Option<Fact>, ()> {
    let Some(receiver) = facts.first() else {
        return Ok(None);
    };
    let Some(ty) = &receiver.ty else {
        return Ok(None);
    };
    let option = |item: &AbiType| {
        Fact::typed(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        })
    };
    let boolean = || Fact::typed(AbiType::Builtin(BuiltinType::Bool));
    let unit = || Fact::typed(AbiType::Builtin(BuiltinType::Unit));
    let input =
        |slot: usize, expected: &AbiType| facts.get(slot).is_some_and(|fact| flows(fact, expected));
    let result = match (operation, ty) {
        (StandardIntrinsic::ArrayPush, AbiType::Array(item, _)) if input(1, item) => {
            Some(receiver.clone())
        }
        (StandardIntrinsic::ArrayInsert, AbiType::Array(item, _)) if input(2, item) => {
            Some(receiver.clone())
        }
        (StandardIntrinsic::ArrayFill, AbiType::Array(item, _)) if input(1, item) => Some(unit()),
        (
            StandardIntrinsic::ArrayGet
            | StandardIntrinsic::ArrayPop
            | StandardIntrinsic::ArrayRemove
            | StandardIntrinsic::ArraySwapRemove,
            AbiType::Array(item, _),
        ) => Some(option(item)),
        (StandardIntrinsic::ArrayClear, AbiType::Array(_, _)) => Some(receiver.clone()),
        (StandardIntrinsic::MapInsert, AbiType::Map { key, value, .. })
            if input(1, key) && input(2, value) =>
        {
            Some(receiver.clone())
        }
        (
            StandardIntrinsic::MapGet | StandardIntrinsic::MapRemove,
            AbiType::Map { key, value, .. },
        ) if input(1, key) => Some(option(value)),
        (StandardIntrinsic::MapContainsKey, AbiType::Map { key, .. }) if input(1, key) => {
            Some(boolean())
        }
        (StandardIntrinsic::MapClear, AbiType::Map { .. }) => Some(receiver.clone()),
        (StandardIntrinsic::SetInsert, AbiType::Set(item, _)) if input(1, item) => {
            Some(receiver.clone())
        }
        (StandardIntrinsic::SetContains | StandardIntrinsic::SetRemove, AbiType::Set(item, _))
            if input(1, item) =>
        {
            Some(boolean())
        }
        (StandardIntrinsic::SetClear, AbiType::Set(_, _)) => Some(receiver.clone()),
        (
            StandardIntrinsic::ArrayPush
            | StandardIntrinsic::ArrayInsert
            | StandardIntrinsic::ArrayFill
            | StandardIntrinsic::ArrayGet
            | StandardIntrinsic::ArrayPop
            | StandardIntrinsic::ArrayRemove
            | StandardIntrinsic::ArraySwapRemove
            | StandardIntrinsic::ArrayClear
            | StandardIntrinsic::MapInsert
            | StandardIntrinsic::MapGet
            | StandardIntrinsic::MapRemove
            | StandardIntrinsic::MapContainsKey
            | StandardIntrinsic::MapClear
            | StandardIntrinsic::SetInsert
            | StandardIntrinsic::SetContains
            | StandardIntrinsic::SetRemove
            | StandardIntrinsic::SetClear,
            _,
        ) => return Err(()),
        _ => None,
    };
    Ok(result)
}
