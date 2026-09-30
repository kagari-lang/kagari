use crate::{
    native_import::{
        NativeSignature,
        contract::{
            bound, builtin, callback, collection_item, enumeration, mutable_array, option,
            readonly_collection, result,
        },
    },
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
    types::{AbiType, GenericBoundAbi},
};
use kagari_common::collection::CollectionAccess;
use std::slice;

pub(super) fn valid(
    operation: StandardIntrinsic,
    signature: &NativeSignature,
    bounds: &[GenericBoundAbi],
) -> bool {
    let p = signature.params.as_slice();
    let out = &signature.result;
    let unit = builtin(BuiltinType::Unit);
    let boolean = builtin(BuiltinType::Bool);
    let string = builtin(BuiltinType::String);
    let index = builtin(BuiltinType::USize);
    match (operation, p) {
        (StandardIntrinsic::DebugAssertEq, [a,b,message]) => a == b && *message == string && *out == unit && bound(bounds,a,StandardTrait::PartialEq).is_some(),
        (StandardIntrinsic::StringBytes, [receiver]) => *receiver == string && *out == AbiType::Iter(Box::new(builtin(BuiltinType::U8))),
        (StandardIntrinsic::StringCharIndices, [receiver]) => *receiver == string && *out == AbiType::Iter(Box::new(AbiType::Tuple(vec![index,string]))),
        (StandardIntrinsic::StringLines | StandardIntrinsic::StringSplitWhitespace, [receiver]) => *receiver == string && *out == AbiType::Iter(Box::new(string)),
        (StandardIntrinsic::StringSplit, [receiver, separator]) => *receiver == string && *separator == string && *out == AbiType::Iter(Box::new(string)),
        (StandardIntrinsic::StringSplitN, [receiver, count, separator]) => *receiver == string && *count == index && *separator == string && *out == AbiType::Iter(Box::new(string)),
        (StandardIntrinsic::StringParse, [receiver]) => {
            let AbiType::StandardEnum {kind: StandardEnum::Result, args} = out else {return false;};
            *receiver == string && args.len() == 2 && bound(bounds,&args[0],StandardTrait::FromStr).is_some() && projection(&args[1], &args[0], StandardTrait::FromStr, "Err")
        }
        (StandardIntrinsic::ArrayListFrom, [input]) => mutable_array(out).is_some_and(|item| readonly_collection(input,StandardTrait::List,slice::from_ref(item))),
        (StandardIntrinsic::LinkedHashSetFrom, [input]) => matches!(out, AbiType::Set(output,CollectionAccess::Mutable) if readonly_collection(input,StandardTrait::List,slice::from_ref(output.as_ref()))),
        (StandardIntrinsic::LinkedHashMapFrom, [input]) => matches!(out, AbiType::Map {key,value,access:CollectionAccess::Mutable} if readonly_collection(input,StandardTrait::List,&[AbiType::Tuple(vec![key.as_ref().clone(), value.as_ref().clone()])])),
        (StandardIntrinsic::ArrayListFromFn, [count, initializer]) => *count == index && mutable_array(out).is_some_and(|item| callback(initializer,slice::from_ref(&index),item)),
        (StandardIntrinsic::ArrayCopyFrom, [receiver,source]) => mutable_array(receiver).is_some_and(|item| readonly_collection(source,StandardTrait::List,slice::from_ref(item))) && *out == unit,
        (StandardIntrinsic::ArrayExtend,[receiver,source]) => mutable_array(receiver).is_some_and(|item| readonly_collection(source,StandardTrait::List,slice::from_ref(item))) && *out == unit,
        (StandardIntrinsic::ArrayRemoveRange, [receiver, range]) => mutable_array(receiver).is_some_and(|item| readonly_collection(out,StandardTrait::List,slice::from_ref(item))) && range_bounds(bounds,range,&index),
        (StandardIntrinsic::ArrayCopyWithin, [receiver, range, destination]) => mutable_array(receiver).is_some() && range_bounds(bounds,range,&index) && *destination == index && *out == unit,
        (StandardIntrinsic::ArrayRetain, [receiver, predicate]) => mutable_array(receiver).is_some_and(|item| callback(predicate,slice::from_ref(item),&boolean)) && *out == unit,
        (StandardIntrinsic::SetRetain, [AbiType::Set(item,CollectionAccess::Mutable), predicate]) => callback(predicate,slice::from_ref(item.as_ref()),&boolean) && *out == unit,
        (StandardIntrinsic::ArraySort | StandardIntrinsic::ArrayDedup, [receiver]) => mutable_array(receiver).is_some_and(|item| bound(bounds,item,if operation == StandardIntrinsic::ArraySort {StandardTrait::Ord} else {StandardTrait::PartialEq}).is_some()) && *out == unit,
        (StandardIntrinsic::ArraySortBy, [receiver, compare]) => mutable_array(receiver).is_some_and(|item| callback(compare,&[item.clone(),item.clone()],&enumeration(StandardEnum::Ordering,vec![]))) && *out == unit,
        (StandardIntrinsic::ArraySortByKey, [receiver, key]) => mutable_array(receiver).is_some_and(|item| matches!(key, AbiType::Function {params,result} if params.as_slice() == slice::from_ref(item) && bound(bounds,result,StandardTrait::Ord).is_some())) && *out == unit,
        (StandardIntrinsic::MapKeys | StandardIntrinsic::MapValues | StandardIntrinsic::MapEntries, [receiver @ AbiType::Map {key,value,..}]) => {
            let item = match operation {StandardIntrinsic::MapKeys => key.as_ref().clone(),StandardIntrinsic::MapValues => value.as_ref().clone(), _ => collection_item(receiver).unwrap()};
            readonly_collection(out,StandardTrait::List,&[item])
        }
        (StandardIntrinsic::MapRetain, [AbiType::Map {key,value,access:CollectionAccess::Mutable}, predicate]) => callback(predicate,&[key.as_ref().clone(),value.as_ref().clone()],&boolean) && *out == unit,
        (StandardIntrinsic::MapGetOrInsertWith | StandardIntrinsic::MapUpdate, [AbiType::Map {key,value,access:CollectionAccess::Mutable}, input, transform]) => input == key.as_ref() && out == value.as_ref() && callback(transform, &if operation == StandardIntrinsic::MapUpdate {vec![option(value)]} else {vec![]}, value),
        (_, [AbiType::StandardEnum {kind:StandardEnum::Option,args}, ..]) if args.len() == 1 => option_operation(operation,p,out,&args[0]),
        (_, [AbiType::StandardEnum {kind:StandardEnum::Result,args}, ..]) if args.len() == 2 => result_operation(operation,p,out,&args[0],&args[1]),
        _ => false,
    }
}

pub(super) fn projection(
    ty: &AbiType,
    receiver: &AbiType,
    kind: StandardTrait,
    name: &str,
) -> bool {
    matches!(ty, AbiType::Projection {receiver: actual, interface, member: id, arguments} if actual.as_ref() == receiver && StandardTrait::from_id(&interface.declaration) == Some(kind) && interface.arguments.is_empty() && arguments.is_empty() && id.path.last().is_some_and(|p| p.name == name) && id.module == interface.declaration.module && id.path[..id.path.len()-1] == interface.declaration.path)
}
fn range_bounds(bounds: &[GenericBoundAbi], receiver: &AbiType, index: &AbiType) -> bool {
    bound(bounds, receiver, StandardTrait::RangeBounds)
        .is_some_and(|interface| interface.arguments.as_slice() == slice::from_ref(index))
}
fn option_operation(op: StandardIntrinsic, p: &[AbiType], out: &AbiType, item: &AbiType) -> bool {
    match (op, p) {
        (StandardIntrinsic::OptionUnwrapOrElse, [_, fallback]) => {
            out == item && callback(fallback, &[], item)
        }
        (StandardIntrinsic::OptionOrElse, [_, fallback]) => {
            *out == option(item) && callback(fallback, &[], out)
        }
        (StandardIntrinsic::OptionMapOr, [_, fallback, transform]) => {
            out == fallback && callback(transform, slice::from_ref(item), out)
        }
        (StandardIntrinsic::OptionMapOrElse, [_, fallback, transform]) => {
            callback(fallback, &[], out) && callback(transform, slice::from_ref(item), out)
        }
        (StandardIntrinsic::OptionFilter | StandardIntrinsic::OptionIsSomeAnd, [_, predicate]) => {
            callback(
                predicate,
                slice::from_ref(item),
                &builtin(BuiltinType::Bool),
            ) && *out
                == if op == StandardIntrinsic::OptionFilter {
                    option(item)
                } else {
                    builtin(BuiltinType::Bool)
                }
        }
        (
            StandardIntrinsic::OptionZip,
            [
                _,
                AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args,
                },
            ],
        ) => {
            args.len() == 1 && *out == option(&AbiType::Tuple(vec![item.clone(), args[0].clone()]))
        }
        (StandardIntrinsic::OptionMap | StandardIntrinsic::OptionAndThen, [_, transform]) => {
            matches!(out, AbiType::StandardEnum {kind:StandardEnum::Option,args} if args.len() == 1 && callback(transform,slice::from_ref(item),if op == StandardIntrinsic::OptionMap {&args[0]} else {out}))
        }
        (StandardIntrinsic::OptionOkOr, [_, error]) => *out == result(item, error),
        (StandardIntrinsic::OptionOkOrElse, [_, transform]) => {
            matches!(out, AbiType::StandardEnum {kind:StandardEnum::Result,args} if args.len() == 2 && &args[0] == item && callback(transform,&[],&args[1]))
        }
        (StandardIntrinsic::OptionFlatten, [_]) => {
            matches!(
                item,
                AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    ..
                }
            ) && out == item
        }
        (StandardIntrinsic::OptionTranspose, [_]) => {
            matches!(item, AbiType::StandardEnum {kind:StandardEnum::Result,args} if args.len() == 2 && *out == result(&option(&args[0]),&args[1]))
        }
        _ => false,
    }
}
fn result_operation(
    op: StandardIntrinsic,
    p: &[AbiType],
    out: &AbiType,
    item: &AbiType,
    error: &AbiType,
) -> bool {
    match (op, p) {
        (StandardIntrinsic::ResultUnwrapOrElse, [_, fallback]) => {
            out == item && callback(fallback, slice::from_ref(error), item)
        }
        (StandardIntrinsic::ResultOrElse, [_, fallback]) => {
            matches!(out, AbiType::StandardEnum {kind:StandardEnum::Result,args} if args.len() == 2 && &args[0] == item && callback(fallback,slice::from_ref(error),out))
        }
        (StandardIntrinsic::ResultMapOr, [_, fallback, transform]) => {
            out == fallback && callback(transform, slice::from_ref(item), out)
        }
        (StandardIntrinsic::ResultMapOrElse, [_, fallback, transform]) => {
            callback(fallback, slice::from_ref(error), out)
                && callback(transform, slice::from_ref(item), out)
        }
        (StandardIntrinsic::ResultOk, [_]) => *out == option(item),
        (StandardIntrinsic::ResultErr, [_]) => *out == option(error),
        (StandardIntrinsic::ResultIsOkAnd | StandardIntrinsic::ResultIsErrAnd, [_, predicate]) => {
            callback(
                predicate,
                slice::from_ref(if op == StandardIntrinsic::ResultIsOkAnd {
                    item
                } else {
                    error
                }),
                &builtin(BuiltinType::Bool),
            ) && *out == builtin(BuiltinType::Bool)
        }
        (StandardIntrinsic::ResultMap | StandardIntrinsic::ResultAndThen, [_, transform]) => {
            matches!(out, AbiType::StandardEnum {kind:StandardEnum::Result,args} if args.len() == 2 && &args[1] == error && callback(transform,slice::from_ref(item),if op == StandardIntrinsic::ResultMap {&args[0]} else {out}))
        }
        (StandardIntrinsic::ResultMapErr, [_, transform]) => {
            matches!(out, AbiType::StandardEnum {kind:StandardEnum::Result,args} if args.len() == 2 && &args[0] == item && callback(transform,slice::from_ref(error),&args[1]))
        }
        (StandardIntrinsic::ResultFlatten, [_]) => {
            matches!(item, AbiType::StandardEnum {kind:StandardEnum::Result,args} if args.len() == 2 && &args[1] == error)
                && out == item
        }
        (StandardIntrinsic::ResultTranspose, [_]) => {
            matches!(item, AbiType::StandardEnum {kind:StandardEnum::Option,args} if args.len() == 1 && *out == option(&result(&args[0],error)))
        }
        _ => false,
    }
}
