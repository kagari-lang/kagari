//! Physical key-operation family consumed by checked native traversal.
use crate::{
    callable::EngineNativeBinding,
    native_import::EngineNativeImport,
    standard::{StandardIntrinsic, bindings::NativeProtocolMethod},
    types::AbiType,
};
pub(super) fn selected(binding: EngineNativeBinding) -> bool {
    matches!(
        binding,
        EngineNativeBinding::Intrinsic(
            StandardIntrinsic::MapGet
                | StandardIntrinsic::MapContainsKey
                | StandardIntrinsic::MapInsert
                | StandardIntrinsic::MapRemove
                | StandardIntrinsic::SetContains
                | StandardIntrinsic::SetInsert
                | StandardIntrinsic::SetRemove
                | StandardIntrinsic::MapGetOrInsertWith
                | StandardIntrinsic::MapUpdate
        )
    )
}
pub(super) fn construction(binding: EngineNativeBinding) -> bool {
    matches!(
        binding,
        EngineNativeBinding::Intrinsic(
            StandardIntrinsic::LinkedHashMapFrom | StandardIntrinsic::LinkedHashSetFrom
        ) | EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator)
    )
}
pub(super) fn key(import: &EngineNativeImport) -> Option<&AbiType> {
    let storage = if selected(import.binding) {
        import.signature.params.first()?
    } else if construction(import.binding) {
        &import.signature.result
    } else {
        return None;
    };
    match storage {
        AbiType::Map { key, .. } | AbiType::Set(key, _) => Some(key),
        _ => None,
    }
}
