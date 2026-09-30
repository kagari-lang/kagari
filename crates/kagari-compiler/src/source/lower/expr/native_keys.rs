//! Selected checked key applications for native storage and remaining producers.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitness,
    standard::{StandardIntrinsic, bindings::NativeProtocolMethod, traits::StandardTrait},
};
use kagari_hir::{
    builtin::traits::StandardTraitSemantics,
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::{TypeId, TypeSubstitution},
};
use kagari_mir::MirValue;

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
        ) {
            Some(result)
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
    pub(super) fn lower_key_storage_call(
        &mut self,
        storage: &TypeId,
        operation: StandardIntrinsic,
        values: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let arguments = match storage {
            TypeId::Map { key, value, .. } => vec![key.as_ref().clone(), value.as_ref().clone()],
            TypeId::Set(key, _) => vec![key.as_ref().clone()],
            _ => return Err(MirLoweringError::MissingBinding("checked key storage")),
        };
        let mut candidates = self.planner.catalog.inherent_methods().filter(|method| {
            if method.function.implementation
                != FunctionImplementation::Native(NativeBinding::Engine(
                    EngineNativeBinding::Intrinsic(operation),
                ))
                || method.function.generic_params.len() != arguments.len()
            {
                return false;
            }
            let substitution: TypeSubstitution = method
                .function
                .generic_params
                .iter()
                .cloned()
                .zip(arguments.iter().cloned())
                .collect();
            method.owner.instantiate(&substitution) == *storage
        });
        let method = candidates
            .next()
            .ok_or(MirLoweringError::MissingBinding(
                "checked native key application",
            ))?
            .clone();
        if candidates.next().is_some() {
            return Err(MirLoweringError::MissingBinding(
                "ambiguous native key application",
            ));
        }
        let substitution: TypeSubstitution = method
            .function
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let result = method.function.return_type.instantiate(&substitution);
        self.lower_native_implementation(&method.declaration, &arguments, &result, values)
    }
}
