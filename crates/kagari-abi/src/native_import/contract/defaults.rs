use crate::{
    native_import::{
        NativeSignature,
        contract::{
            bound, builtin, callback, iterable_item, member, option, readonly_collection, result,
        },
    },
    scalar::BuiltinType,
    standard::{
        bindings::NativeDefaultMethod, intrinsic, surface::StandardEnum, traits::StandardTrait,
    },
    types::{AbiType, GenericBoundAbi},
};
use kagari_common::{collection::CollectionAccess, identity::associated_type_id};
use std::slice;

pub(super) fn valid(
    method: NativeDefaultMethod,
    signature: &NativeSignature,
    bounds: &[GenericBoundAbi],
) -> bool {
    let Some(receiver) = signature.params.first() else {
        return false;
    };
    let AbiType::SelfType(owner) = receiver else {
        if matches!(
            method,
            NativeDefaultMethod::MapKeysView
                | NativeDefaultMethod::MapValuesView
                | NativeDefaultMethod::MapEntriesView
        ) {
            return bound(bounds, receiver, StandardTrait::Map).is_some_and(|interface| {
                let [key, value] = interface.arguments.as_slice() else {
                    return false;
                };
                let item = match method {
                    NativeDefaultMethod::MapKeysView => key.clone(),
                    NativeDefaultMethod::MapValuesView => value.clone(),
                    _ => AbiType::Tuple(vec![key.clone(), value.clone()]),
                };
                signature.params.len() == 1
                    && readonly_collection(&signature.result, StandardTrait::List, &[item])
            });
        }
        if matches!(
            method,
            NativeDefaultMethod::ListFirst
                | NativeDefaultMethod::ListLast
                | NativeDefaultMethod::ListBinarySearch
                | NativeDefaultMethod::ListContains
                | NativeDefaultMethod::ListStartsWith
                | NativeDefaultMethod::ListEndsWith
        ) {
            return bound(bounds, receiver, StandardTrait::List).is_some_and(|interface| {
                let [item] = interface.arguments.as_slice() else {
                    return false;
                };
                let index = builtin(BuiltinType::USize);
                match (method, signature.params.as_slice()) {
                    (NativeDefaultMethod::ListFirst | NativeDefaultMethod::ListLast, [_]) => {
                        signature.result == option(item)
                    }
                    (NativeDefaultMethod::ListBinarySearch, [_, value]) => {
                        value == item
                            && signature.result == result(&index, &index)
                            && bound(bounds, item, StandardTrait::Ord).is_some()
                    }
                    (NativeDefaultMethod::ListContains, [_, value]) => {
                        value == item
                            && signature.result == builtin(BuiltinType::Bool)
                            && bound(bounds, item, StandardTrait::PartialEq).is_some()
                    }
                    (
                        NativeDefaultMethod::ListStartsWith | NativeDefaultMethod::ListEndsWith,
                        [_, prefix],
                    ) => {
                        readonly_collection(prefix, StandardTrait::List, slice::from_ref(item))
                            && signature.result == builtin(BuiltinType::Bool)
                            && bound(bounds, item, StandardTrait::PartialEq).is_some()
                    }
                    _ => false,
                }
            });
        }
        if matches!(receiver, AbiType::Host(_) | AbiType::Trait(_)) {
            return false;
        }
        return bound(bounds, receiver, StandardTrait::Iterator)
            .and_then(|interface| member(interface, "Item"))
            .is_some_and(|item| iterator(method, signature, bounds, item));
    };
    let Some(kind) = StandardTrait::from_id(owner) else {
        return false;
    };
    match kind {
        StandardTrait::Iterator => {
            let interface = intrinsic::applied(kind, vec![]);
            let item = AbiType::Projection {
                receiver: Box::new(receiver.clone()),
                member: associated_type_id(&interface.declaration, "Item"),
                interface: Box::new(interface),
                arguments: vec![],
            };
            iterator(method, signature, bounds, &item)
        }
        StandardTrait::List | StandardTrait::Set | StandardTrait::Map => {
            let item = AbiType::Parameter {
                owner: owner.clone(),
                position: 0,
            };
            let p = signature.params.as_slice();
            let out = &signature.result;
            let index = builtin(BuiltinType::USize);
            let string = builtin(BuiltinType::String);
            let boolean = builtin(BuiltinType::Bool);
            match (kind, method, p) {
                (StandardTrait::List, NativeDefaultMethod::ListJoin, [_, separator]) => {
                    *separator == string
                        && *out == string
                        && bound(bounds, receiver, StandardTrait::Iterable)
                            .and_then(|interface| member(interface, "Item"))
                            == Some(&string)
                }
                (
                    StandardTrait::List,
                    NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks,
                    [_, size],
                ) => {
                    *size == index
                        && *out
                            == iter(AbiType::Trait(intrinsic::applied(
                                StandardTrait::List,
                                vec![item],
                            )))
                }
                (
                    StandardTrait::List,
                    NativeDefaultMethod::ListFirst | NativeDefaultMethod::ListLast,
                    [_],
                ) => *out == option(&item),
                (StandardTrait::List, NativeDefaultMethod::ListContains, [_, value]) => {
                    *value == item
                        && *out == boolean
                        && bound(bounds, &item, StandardTrait::PartialEq).is_some()
                }
                (
                    StandardTrait::List,
                    NativeDefaultMethod::ListStartsWith | NativeDefaultMethod::ListEndsWith,
                    [_, prefix],
                ) => {
                    readonly_collection(prefix, StandardTrait::List, slice::from_ref(&item))
                        && *out == boolean
                        && bound(bounds, &item, StandardTrait::PartialEq).is_some()
                }
                (StandardTrait::List, NativeDefaultMethod::ListBinarySearch, [_, value]) => {
                    *value == item
                        && *out == result(&index, &index)
                        && bound(bounds, &item, StandardTrait::Ord).is_some()
                }
                (
                    StandardTrait::Set,
                    NativeDefaultMethod::SetUnion
                    | NativeDefaultMethod::SetIntersection
                    | NativeDefaultMethod::SetDifference
                    | NativeDefaultMethod::SetSymmetricDifference,
                    [_, rhs],
                ) => {
                    readonly_collection(rhs, StandardTrait::Set, slice::from_ref(&item))
                        && *out == AbiType::Set(Box::new(item.clone()), CollectionAccess::Mutable)
                        && bound(bounds, &item, StandardTrait::Eq).is_some()
                        && bound(bounds, &item, StandardTrait::Hash).is_some()
                }
                (
                    StandardTrait::Set,
                    NativeDefaultMethod::SetIsSubset
                    | NativeDefaultMethod::SetIsSuperset
                    | NativeDefaultMethod::SetIsDisjoint,
                    [_, rhs],
                ) => {
                    readonly_collection(rhs, StandardTrait::Set, slice::from_ref(&item))
                        && *out == boolean
                }
                (
                    StandardTrait::Map,
                    NativeDefaultMethod::MapKeysView
                    | NativeDefaultMethod::MapValuesView
                    | NativeDefaultMethod::MapEntriesView,
                    [_],
                ) => {
                    let value = AbiType::Parameter {
                        owner: owner.clone(),
                        position: 1,
                    };
                    let output = match method {
                        NativeDefaultMethod::MapKeysView => item,
                        NativeDefaultMethod::MapValuesView => value,
                        _ => AbiType::Tuple(vec![item, value]),
                    };
                    readonly_collection(out, StandardTrait::List, &[output])
                }
                _ => false,
            }
        }
        _ => false,
    }
}
fn iter(item: AbiType) -> AbiType {
    AbiType::Iter(Box::new(item))
}
fn iterator(
    method: NativeDefaultMethod,
    signature: &NativeSignature,
    bounds: &[GenericBoundAbi],
    item: &AbiType,
) -> bool {
    let p = signature.params.as_slice();
    let out = &signature.result;
    let index = builtin(BuiltinType::USize);
    let boolean = builtin(BuiltinType::Bool);
    let unit = builtin(BuiltinType::Unit);
    let string = builtin(BuiltinType::String);
    match (method, p) {
        (NativeDefaultMethod::Join, [receiver, separator]) => {
            *separator == string
                && *out == string
                && bound(bounds, receiver, StandardTrait::Iterator)
                    .and_then(|interface| member(interface, "Item"))
                    == Some(&string)
        }
        (
            NativeDefaultMethod::Collect | NativeDefaultMethod::Sum | NativeDefaultMethod::Product,
            [_],
        ) => bound(
            bounds,
            out,
            match method {
                NativeDefaultMethod::Collect => StandardTrait::FromIterator,
                NativeDefaultMethod::Sum => StandardTrait::Sum,
                _ => StandardTrait::Product,
            },
        )
        .is_some_and(|interface| interface.arguments.as_slice() == slice::from_ref(item)),
        (NativeDefaultMethod::Map, [_, transform]) => {
            matches!(out, AbiType::Iter(output) if callback(transform,slice::from_ref(item),output))
        }
        (
            NativeDefaultMethod::Filter
            | NativeDefaultMethod::TakeWhile
            | NativeDefaultMethod::SkipWhile,
            [_, predicate],
        ) => *out == iter(item.clone()) && callback(predicate, slice::from_ref(item), &boolean),
        (NativeDefaultMethod::Inspect, [_, callback_ty]) => {
            *out == iter(item.clone()) && callback(callback_ty, slice::from_ref(item), &unit)
        }
        (NativeDefaultMethod::FilterMap, [_, transform]) => {
            matches!(out, AbiType::Iter(output) if callback(transform,slice::from_ref(item),&option(output)))
        }
        (NativeDefaultMethod::Take | NativeDefaultMethod::Skip, [_, count]) => {
            *count == index && *out == iter(item.clone())
        }
        (NativeDefaultMethod::Enumerate, [_]) => {
            *out == iter(AbiType::Tuple(vec![index, item.clone()]))
        }
        (NativeDefaultMethod::Zip, [_, source]) => {
            bound(bounds, source, StandardTrait::Iterable).is_some()
                && matches!(out, AbiType::Iter(output) if matches!(output.as_ref(),AbiType::Tuple(items) if items.len()==2 && &items[0] == item && super::callbacks::projection(&items[1],source,StandardTrait::Iterable,"Item")))
        }
        (NativeDefaultMethod::Chain, [_, source]) => {
            iterable_item(bounds, source) == Some(item) && *out == iter(item.clone())
        }
        (
            NativeDefaultMethod::Find
            | NativeDefaultMethod::Any
            | NativeDefaultMethod::All
            | NativeDefaultMethod::Position,
            [_, predicate],
        ) => {
            callback(predicate, slice::from_ref(item), &boolean)
                && *out
                    == match method {
                        NativeDefaultMethod::Find => option(item),
                        NativeDefaultMethod::Position => option(&index),
                        _ => boolean,
                    }
        }
        (NativeDefaultMethod::Count, [_]) => *out == index,
        (NativeDefaultMethod::Fold, [_, initial, combine]) => {
            out == initial && callback(combine, &[initial.clone(), item.clone()], out)
        }
        (NativeDefaultMethod::ForEach, [_, visit]) => {
            *out == unit && callback(visit, slice::from_ref(item), &unit)
        }
        (NativeDefaultMethod::Partition, [_, predicate]) => {
            callback(predicate, slice::from_ref(item), &boolean)
                && matches!(out, AbiType::Tuple(outputs) if matches!(outputs.as_slice(),[a,b] if a==b && bound(bounds,a,StandardTrait::FromIterator).is_some_and(|interface| interface.arguments.as_slice()==slice::from_ref(item))))
        }
        (NativeDefaultMethod::GroupBy, [_, key_fn]) => {
            matches!(out, AbiType::Map {key,value,access:CollectionAccess::Mutable} if callback(key_fn,slice::from_ref(item),key) && **value == AbiType::Array(Box::new(item.clone()),CollectionAccess::Mutable) && bound(bounds,key,StandardTrait::Eq).is_some() && bound(bounds,key,StandardTrait::Hash).is_some())
        }
        (NativeDefaultMethod::Fuse, [_]) => *out == iter(item.clone()),
        (NativeDefaultMethod::FindMap, [_, transform]) => {
            matches!(out, AbiType::StandardEnum {kind:StandardEnum::Option,args} if args.len()==1 && callback(transform,slice::from_ref(item),out))
        }
        (NativeDefaultMethod::Nth, [_, n]) => *n == index && *out == option(item),
        (NativeDefaultMethod::Last, [_]) => *out == option(item),
        (NativeDefaultMethod::Reduce, [_, combine]) => {
            *out == option(item) && callback(combine, &[item.clone(), item.clone()], item)
        }
        (NativeDefaultMethod::Min | NativeDefaultMethod::Max, [_]) => {
            *out == option(item) && bound(bounds, item, StandardTrait::Ord).is_some()
        }
        (NativeDefaultMethod::MinBy | NativeDefaultMethod::MaxBy, [_, compare]) => {
            *out == option(item)
                && callback(
                    compare,
                    &[item.clone(), item.clone()],
                    &super::enumeration(StandardEnum::Ordering, vec![]),
                )
        }
        (NativeDefaultMethod::MinByKey | NativeDefaultMethod::MaxByKey, [_, key_fn]) => {
            *out == option(item)
                && matches!(key_fn, AbiType::Function {params,result} if params.as_slice()==slice::from_ref(item) && bound(bounds,result,StandardTrait::Ord).is_some())
        }
        (NativeDefaultMethod::FlatMap, [_, transform]) => {
            matches!(transform, AbiType::Function {params,result} if params.as_slice()==slice::from_ref(item) && bound(bounds,result,StandardTrait::Iterable).is_some() && matches!(out, AbiType::Iter(output) if super::callbacks::projection(output,result,StandardTrait::Iterable,"Item")))
        }
        (NativeDefaultMethod::Flatten, [_]) => {
            bound(bounds, item, StandardTrait::Iterable).is_some()
                && matches!(out, AbiType::Iter(output) if super::callbacks::projection(output,item,StandardTrait::Iterable,"Item"))
        }
        _ => false,
    }
}
