//! Carry selected nested destination applications without expanding their algorithms.
use crate::source::{
    lower::{MirLoweringError, abi::checked_bounds, state::FunctionLowerer},
    types::{raise_nominal_type, raise_type},
};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitness,
    standard::{
        bindings::{NativeDefaultMethod, NativeProtocolMethod},
        traits::StandardTrait,
    },
    types::substitution::TypeSubstitution,
};
use kagari_common::collection::CollectionAccess;
use kagari_hir::{
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::{TypeId, abi::lower_type},
};

impl FunctionLowerer<'_, '_> {
    pub(super) fn native_destinations(
        &mut self,
        binding: EngineNativeBinding,
        params: &[TypeId],
        result: &TypeId,
        witnesses: &mut Vec<NativeWitness>,
    ) -> Result<(), MirLoweringError> {
        if !matches!(
            binding,
            EngineNativeBinding::Protocol(
                NativeProtocolMethod::OptionFromIterator | NativeProtocolMethod::ResultFromIterator
            ) | EngineNativeBinding::TraitDefault(
                NativeDefaultMethod::Collect | NativeDefaultMethod::Partition
            )
        ) {
            return Ok(());
        }
        let invalid =
            || MirLoweringError::MissingBinding("checked nested FromIterator application");
        let mut index = 0;
        while index < witnesses.len() {
            if witnesses.len() > self.planner.options.max_type_nodes {
                return Err(invalid());
            }
            let witness = witnesses[index].clone();
            index += 1;
            if StandardTrait::from_id(&witness.interface.declaration)
                != Some(StandardTrait::FromIterator)
            {
                continue;
            }
            let receiver = raise_type(&witness.receiver);
            let interface = raise_nominal_type(&witness.interface);
            let Some((declaration, mut arguments)) = self.planner.catalog.implementation_method(
                &self.protocol_method(StandardTrait::FromIterator, 0)?,
                &interface,
                &receiver,
            ) else {
                return Err(invalid());
            };
            let Some(function) = self.planner.native_function(&declaration).cloned() else {
                continue;
            };
            let FunctionImplementation::Native(NativeBinding::Engine(provider)) =
                function.implementation
            else {
                return Err(invalid());
            };
            let [item] = interface.arguments.as_slice() else {
                return Err(invalid());
            };
            let source = if binding
                == EngineNativeBinding::TraitDefault(NativeDefaultMethod::Collect)
                && witness.receiver == lower_type(result)
            {
                params.first().ok_or_else(invalid)?.clone()
            } else {
                TypeId::Array(Box::new(item.clone()), CollectionAccess::Mutable)
            };
            arguments.push(source.clone());
            if arguments.len() != function.generic_params.len() {
                return Err(invalid());
            }
            let mut substitution = TypeSubstitution::default();
            let arguments: Vec<_> = arguments.iter().map(lower_type).collect();
            for (parameter, argument) in function.generic_params.iter().zip(&arguments) {
                substitution.bind(&parameter.owner, parameter.position, argument);
            }
            let requirements = substitution
                .apply_bounds(
                    &checked_bounds(&function.bounds),
                    &self.planner.options.cancel,
                )
                .map_err(|_| invalid())?;
            for witness in self.native_requirement_witnesses(provider, &requirements)? {
                if !witnesses.contains(&witness) {
                    witnesses.push(witness);
                }
            }
            self.native_key_witnesses(provider, &[source], &receiver, witnesses)?;
        }
        Ok(())
    }
}
