//! Selected checked key applications for native storage and remaining producers.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitness,
    standard::{
        StandardIntrinsic,
        bindings::{NativeDefaultMethod, NativeProtocolMethod},
        traits::StandardTrait,
    },
};
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn key_binding(&self, binding: EngineNativeBinding) -> bool {
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
    pub(super) fn native_key_witnesses(
        &mut self,
        binding: EngineNativeBinding,
        params: &[TypeId],
        result: &TypeId,
        witnesses: &mut Vec<NativeWitness>,
    ) -> Result<(), MirLoweringError> {
        let storage = if self.key_binding(binding) {
            params.first()
        } else if matches!(
            binding,
            EngineNativeBinding::Intrinsic(
                StandardIntrinsic::LinkedHashMapFrom | StandardIntrinsic::LinkedHashSetFrom
            ) | EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator)
        ) || binding == EngineNativeBinding::TraitDefault(NativeDefaultMethod::GroupBy)
        {
            Some(result)
        } else if self.set_binding(binding) {
            if matches!(result, TypeId::Set(..)) {
                Some(result)
            } else {
                params.first()
            }
        } else {
            None
        };
        let key = match storage {
            Some(TypeId::Map { key, .. } | TypeId::Set(key, _)) => key,
            _ => return Ok(()),
        };
        for protocol in [
            StandardTrait::Eq,
            StandardTrait::Hash,
            StandardTrait::PartialEq,
        ] {
            let witness = self.lower_native_witness(key, &protocol.nominal(), &[])?;
            if !witnesses.contains(&witness) {
                witnesses.push(witness);
            }
        }
        Ok(())
    }
}
