//! Direct string and collection storage guards. Container elements are invariant;
//! only a caller's outer mutable capability may weaken to a readonly view.
use crate::{
    native_import::signature::enumeration,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum},
    types::AbiType,
};
use kagari_common::collection::CollectionAccess;
use std::slice;

pub(super) fn valid(operation: StandardIntrinsic, params: &[AbiType], result: &AbiType) -> bool {
    let unit = AbiType::Builtin(BuiltinType::Unit);
    let boolean = AbiType::Builtin(BuiltinType::Bool);
    let index = AbiType::Builtin(BuiltinType::USize);
    let string = AbiType::Builtin(BuiltinType::String);
    match (operation, params) {
        (StandardIntrinsic::StringLenBytes | StandardIntrinsic::StringLenChars, [receiver]) => {
            receiver == &string && result == &index
        }
        (StandardIntrinsic::StringIsEmpty | StandardIntrinsic::StringIsAscii, [receiver]) => {
            receiver == &string && result == &boolean
        }
        (
            StandardIntrinsic::StringContains
            | StandardIntrinsic::StringStartsWith
            | StandardIntrinsic::StringEndsWith
            | StandardIntrinsic::StringEqIgnoreAsciiCase,
            [receiver, other],
        ) => receiver == &string && other == &string && result == &boolean,
        (StandardIntrinsic::StringConcat, [receiver, other]) => {
            receiver == &string && other == &string && result == &string
        }
        (StandardIntrinsic::StringReplace, [receiver, from, to]) => {
            receiver == &string && from == &string && to == &string && result == &string
        }
        (StandardIntrinsic::StringReplaceN, [receiver, from, to, count]) => {
            receiver == &string
                && from == &string
                && to == &string
                && count == &index
                && result == &string
        }
        (StandardIntrinsic::StringRepeat, [receiver, count]) => {
            receiver == &string && count == &index && result == &string
        }
        (StandardIntrinsic::StringIsCharBoundary, [receiver, position]) => {
            receiver == &string && position == &index && result == &boolean
        }
        (
            StandardIntrinsic::StringToAsciiLowercase
            | StandardIntrinsic::StringToAsciiUppercase
            | StandardIntrinsic::StringToLowercase
            | StandardIntrinsic::StringToUppercase
            | StandardIntrinsic::StringTrim
            | StandardIntrinsic::StringTrimStart
            | StandardIntrinsic::StringTrimEnd,
            [receiver],
        ) => receiver == &string && result == &string,
        (StandardIntrinsic::StringFind | StandardIntrinsic::StringRfind, [receiver, pattern]) => {
            receiver == &string
                && pattern == &string
                && enumeration(result, StandardEnum::Option, slice::from_ref(&index))
        }
        (
            StandardIntrinsic::StringStripPrefix | StandardIntrinsic::StringStripSuffix,
            [receiver, pattern],
        ) => {
            receiver == &string
                && pattern == &string
                && enumeration(result, StandardEnum::Option, slice::from_ref(&string))
        }
        (
            StandardIntrinsic::StringSplitOnce | StandardIntrinsic::StringRsplitOnce,
            [receiver, pattern],
        ) => {
            receiver == &string
                && pattern == &string
                && enumeration(
                    result,
                    StandardEnum::Option,
                    &[AbiType::Tuple(vec![string.clone(), string])],
                )
        }
        (StandardIntrinsic::StringSlice, [receiver, start, end]) => {
            receiver == &string
                && start == &index
                && end == &index
                && enumeration(result, StandardEnum::Option, slice::from_ref(&string))
        }
        (StandardIntrinsic::ArrayListNew, [])
        | (StandardIntrinsic::ArrayWithCapacity, [AbiType::Builtin(BuiltinType::USize)]) => {
            matches!(result, AbiType::Array(_, CollectionAccess::Mutable))
        }
        (StandardIntrinsic::LinkedHashMapNew, [])
        | (StandardIntrinsic::MapWithCapacity, [AbiType::Builtin(BuiltinType::USize)]) => matches!(
            result,
            AbiType::Map {
                access: CollectionAccess::Mutable,
                ..
            }
        ),
        (StandardIntrinsic::LinkedHashSetNew, [])
        | (StandardIntrinsic::SetWithCapacity, [AbiType::Builtin(BuiltinType::USize)]) => {
            matches!(result, AbiType::Set(_, CollectionAccess::Mutable))
        }
        (
            StandardIntrinsic::ArrayLen | StandardIntrinsic::ArrayCapacity,
            [AbiType::Array(_, _)],
        )
        | (StandardIntrinsic::MapLen | StandardIntrinsic::MapCapacity, [AbiType::Map { .. }])
        | (StandardIntrinsic::SetLen | StandardIntrinsic::SetCapacity, [AbiType::Set(_, _)]) => {
            result == &index
        }
        (StandardIntrinsic::ArrayIsEmpty, [AbiType::Array(_, _)])
        | (StandardIntrinsic::MapIsEmpty, [AbiType::Map { .. }])
        | (StandardIntrinsic::SetIsEmpty, [AbiType::Set(_, _)]) => result == &boolean,
        (StandardIntrinsic::ArrayGet, [AbiType::Array(item, _), position]) => {
            position == &index
                && enumeration(result, StandardEnum::Option, slice::from_ref(item.as_ref()))
        }
        (StandardIntrinsic::ArrayPop, [AbiType::Array(item, CollectionAccess::Mutable)]) => {
            enumeration(result, StandardEnum::Option, slice::from_ref(item.as_ref()))
        }
        (
            StandardIntrinsic::ArrayRemove | StandardIntrinsic::ArraySwapRemove,
            [AbiType::Array(item, CollectionAccess::Mutable), position],
        ) => {
            position == &index
                && enumeration(result, StandardEnum::Option, slice::from_ref(item.as_ref()))
        }
        (
            StandardIntrinsic::ArrayPush,
            [
                receiver @ AbiType::Array(item, CollectionAccess::Mutable),
                value,
            ],
        ) => value == item.as_ref() && (result == receiver || receiver.can_weaken_to(result)),
        (
            StandardIntrinsic::ArrayInsert,
            [
                receiver @ AbiType::Array(item, CollectionAccess::Mutable),
                position,
                value,
            ],
        ) => {
            position == &index
                && value == item.as_ref()
                && (result == receiver || receiver.can_weaken_to(result))
        }
        (
            StandardIntrinsic::ArrayClear,
            [receiver @ AbiType::Array(_, CollectionAccess::Mutable)],
        ) => result == receiver || receiver.can_weaken_to(result),
        (
            StandardIntrinsic::ArrayFill,
            [AbiType::Array(item, CollectionAccess::Mutable), value],
        ) => value == item.as_ref() && result == &unit,
        (StandardIntrinsic::ArraySwap, [AbiType::Array(_, CollectionAccess::Mutable), a, b]) => {
            a == &index && b == &index && result == &unit
        }
        (StandardIntrinsic::ArrayReverse, [AbiType::Array(_, CollectionAccess::Mutable)]) => {
            result == &unit
        }
        (
            StandardIntrinsic::ArrayReserve | StandardIntrinsic::ArrayTruncate,
            [AbiType::Array(_, CollectionAccess::Mutable), count],
        )
        | (
            StandardIntrinsic::MapReserve,
            [
                AbiType::Map {
                    access: CollectionAccess::Mutable,
                    ..
                },
                count,
            ],
        )
        | (StandardIntrinsic::SetReserve, [AbiType::Set(_, CollectionAccess::Mutable), count]) => {
            count == &index && result == &unit
        }
        (StandardIntrinsic::MapContainsKey, [AbiType::Map { key, .. }, value]) => {
            value == key.as_ref() && result == &boolean
        }
        (StandardIntrinsic::MapGet, [AbiType::Map { key, value, .. }, input])
        | (
            StandardIntrinsic::MapRemove,
            [
                AbiType::Map {
                    key,
                    value,
                    access: CollectionAccess::Mutable,
                },
                input,
            ],
        ) => {
            input == key.as_ref()
                && enumeration(
                    result,
                    StandardEnum::Option,
                    slice::from_ref(value.as_ref()),
                )
        }
        (
            StandardIntrinsic::MapInsert,
            [
                receiver @ AbiType::Map {
                    key,
                    value,
                    access: CollectionAccess::Mutable,
                },
                input,
                item,
            ],
        ) => {
            input == key.as_ref()
                && item == value.as_ref()
                && (result == receiver || receiver.can_weaken_to(result))
        }
        (
            StandardIntrinsic::MapClear,
            [
                receiver @ AbiType::Map {
                    access: CollectionAccess::Mutable,
                    ..
                },
            ],
        )
        | (StandardIntrinsic::SetClear, [receiver @ AbiType::Set(_, CollectionAccess::Mutable)]) => {
            result == receiver || receiver.can_weaken_to(result)
        }
        (StandardIntrinsic::SetContains, [AbiType::Set(item, _), value])
        | (StandardIntrinsic::SetRemove, [AbiType::Set(item, CollectionAccess::Mutable), value]) => {
            value == item.as_ref() && result == &boolean
        }
        (
            StandardIntrinsic::SetInsert,
            [
                receiver @ AbiType::Set(item, CollectionAccess::Mutable),
                value,
            ],
        ) => value == item.as_ref() && (result == receiver || receiver.can_weaken_to(result)),
        (StandardIntrinsic::SetToArray, [AbiType::Set(item, _)]) => {
            matches!(result, AbiType::Array(output, CollectionAccess::Mutable) if item == output)
        }
        (StandardIntrinsic::ArrayJoin, [AbiType::Array(item, _), separator]) => {
            item.as_ref() == &string && separator == &string && result == &string
        }
        (
            StandardIntrinsic::OptionIsSome | StandardIntrinsic::OptionIsNone,
            [
                AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args,
                },
            ],
        ) => args.len() == 1 && result == &boolean,
        (
            StandardIntrinsic::ResultIsOk | StandardIntrinsic::ResultIsErr,
            [
                AbiType::StandardEnum {
                    kind: StandardEnum::Result,
                    args,
                },
            ],
        ) => args.len() == 2 && result == &boolean,
        (
            StandardIntrinsic::OptionUnwrapOr,
            [
                AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args,
                },
                fallback,
            ],
        ) => args.as_slice() == slice::from_ref(fallback) && result == fallback,
        (
            StandardIntrinsic::ResultUnwrapOr,
            [
                AbiType::StandardEnum {
                    kind: StandardEnum::Result,
                    args,
                },
                fallback,
            ],
        ) => args.len() == 2 && &args[0] == fallback && result == fallback,
        _ => false,
    }
}
