//! Closed primitive entrypoints selected from checked native bindings.
use crate::{
    callable::EngineNativeBinding,
    native_import::{EngineCoreOperation, NativeSignature, contract},
    operations::{self, IterOp},
    standard::bindings::NativeProtocolMethod,
};

pub(super) fn resolve(
    binding: EngineNativeBinding,
    signature: &NativeSignature,
) -> Option<EngineCoreOperation> {
    let EngineNativeBinding::Protocol(method) = binding else {
        return None;
    };
    if !contract::binding_signature_valid(binding, signature, &[]) {
        return None;
    }
    let [source] = signature.params.as_slice() else {
        return None;
    };
    match method {
        NativeProtocolMethod::CollectionIter => IterOp::New
            .contract(source)
            .map(|_| EngineCoreOperation::IterNew),
        NativeProtocolMethod::IterNext => IterOp::Next
            .contract(source)
            .map(|_| EngineCoreOperation::IterNext),
        NativeProtocolMethod::RangeStartBound | NativeProtocolMethod::RangeEndBound
            if operations::range_bound_valid(source, &signature.result) =>
        {
            Some(if method == NativeProtocolMethod::RangeStartBound {
                EngineCoreOperation::RangeStartBound
            } else {
                EngineCoreOperation::RangeEndBound
            })
        }
        _ => None,
    }
}
