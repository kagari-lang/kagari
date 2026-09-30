use crate::{
    native_import::{
        NativeSignature,
        contract::{bound, builtin, collection_item, enumeration, iterable_item, option},
    },
    numeric,
    scalar::BuiltinType,
    standard::{bindings::NativeProtocolMethod, surface::StandardEnum, traits::StandardTrait},
    types::{AbiType, GenericBoundAbi},
};

pub(super) fn valid(
    method: NativeProtocolMethod,
    signature: &NativeSignature,
    bounds: &[GenericBoundAbi],
) -> bool {
    let output = &signature.result;
    match (method, signature.params.as_slice()) {
        (NativeProtocolMethod::CollectionIter, [receiver]) => {
            collection_item(receiver).is_some_and(|item| *output == AbiType::Iter(Box::new(item)))
        }
        (NativeProtocolMethod::IterNext, [AbiType::Iter(item)]) => *output == option(item),
        (NativeProtocolMethod::CollectionSet, [receiver, index, value]) => {
            super::mutable_array(receiver) == Some(value)
                && *index == builtin(BuiltinType::USize)
                && *output == builtin(BuiltinType::Unit)
        }
        (
            NativeProtocolMethod::RangeStartBound | NativeProtocolMethod::RangeEndBound,
            [AbiType::Range(item, kind)],
        ) => {
            matches!(output, AbiType::StandardEnum {kind: StandardEnum::Bound, args} if args.len() == 1 && (*kind == RangeKind::Full || args[0] == **item))
        }
        (NativeProtocolMethod::NumericFromStr, [input]) => {
            let AbiType::StandardEnum {
                kind: StandardEnum::Result,
                args,
            } = output
            else {
                return false;
            };
            matches!(args.as_slice(), [AbiType::Builtin(scalar), error] if *input == builtin(BuiltinType::String) && numeric::parsing_error(*scalar).is_some_and(|kind| *error == enumeration(kind, vec![])))
        }
        (NativeProtocolMethod::CollectionFromIterator, [source]) => {
            collection_item(output).is_some_and(|item| iterable_item(bounds, source) == Some(&item))
        }
        (
            NativeProtocolMethod::OptionFromIterator | NativeProtocolMethod::ResultFromIterator,
            [source],
        ) => {
            let kind = if method == NativeProtocolMethod::OptionFromIterator {
                StandardEnum::Option
            } else {
                StandardEnum::Result
            };
            let (
                AbiType::StandardEnum {
                    kind: actual,
                    args: outputs,
                },
                Some(AbiType::StandardEnum {
                    kind: input_kind,
                    args: inputs,
                }),
            ) = (output, iterable_item(bounds, source))
            else {
                return false;
            };
            *actual == kind
                && *input_kind == kind
                && outputs.len() == kind.arity()
                && inputs.len() == kind.arity()
                && (kind != StandardEnum::Result || outputs[1] == inputs[1])
                && bound(bounds, &outputs[0], StandardTrait::FromIterator).is_some_and(
                    |interface| interface.arguments.as_slice() == slice::from_ref(&inputs[0]),
                )
        }
        _ => false,
    }
}

use kagari_common::range::RangeKind;
use std::slice;
