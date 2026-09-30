//! Physical key-operation family consumed by checked native traversal.
use crate::{
    callable::EngineNativeBinding, native_import::EngineNativeImport, standard::StandardIntrinsic,
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
pub(super) fn key(import: &EngineNativeImport) -> Option<&AbiType> {
    if !selected(import.binding) {
        return None;
    }
    match import.signature.params.first()? {
        AbiType::Map { key, .. } | AbiType::Set(key, _) => Some(key),
        _ => None,
    }
}
