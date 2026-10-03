//! Validate generated protocol functions against carried declarations and bounds.
use crate::{
    callable::CallableImplementation,
    effects::EffectSet,
    native_import::{
        NativeSignature,
        callables::{NativeCallableApplication, NativeCallableOrigin, NativeCallableRequirement},
        protocol::{adapter_arguments, adapter_contract},
    },
    types::{
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};

impl ProofCatalog<'_> {
    pub fn implicit_callable_signature(
        &self,
        required: &NativeCallableRequirement,
        cancel: &CancellationToken,
    ) -> Result<Option<NativeSignature>, TypeTransformError> {
        let Some((_, expected)) = adapter_contract(required) else {
            return Ok(None);
        };
        if !self.callable_requirement_valid(required)
            || !self.holds(&required.interface, &required.receiver, &[], cancel)?
            || self.has_explicit_implementation(&required.interface, &required.receiver, cancel)?
        {
            return Ok(None);
        }
        let Some(contract) = self.contracts.get(&required.interface.declaration) else {
            return Ok(None);
        };
        let Some(method) = contract.methods.iter().find(|method| {
            required
                .member
                .path
                .last()
                .is_some_and(|member| member.name == method.name)
        }) else {
            return Ok(None);
        };
        if contract.generic_params.len() != required.interface.arguments.len()
            || !method.generic_params.is_empty()
        {
            return Ok(None);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in contract
            .generic_params
            .iter()
            .zip(&required.interface.arguments)
        {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        substitution.bind_receiver(&required.interface.declaration, &required.receiver);
        for bound in substitution.apply_bounds(&method.bounds, cancel)? {
            if !self.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(None);
            }
        }
        let normalize = |ty| self.normalize(&substitution.apply(ty, cancel)?, cancel);
        let applied = NativeSignature {
            params: method
                .params
                .iter()
                .map(|param| normalize(&param.ty))
                .collect::<Result<_, _>>()?,
            result: normalize(&method.return_type)?,
        };
        Ok((applied == expected).then_some(applied))
    }

    pub fn callable_matches(
        &self,
        required: &NativeCallableRequirement,
        selected: &NativeCallableApplication,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        if selected.origin == NativeCallableOrigin::Implementation {
            return Ok(self.select_callable(required, cancel)?.as_ref() == Some(selected));
        }
        let Some((kind, _)) = adapter_contract(required) else {
            return Ok(false);
        };
        let declaration = &selected.instance.declaration;
        Ok(selected.requirement == *required
            && selected.implementation == CallableImplementation::Script
            && selected.effects == EffectSet::native_call()
            && selected.instance.arguments == adapter_arguments(required)
            && declaration.path.len() == 1
            && declaration.path[0].kind == DefinitionKind::Function
            && declaration.path[0].occurrence == 0
            && declaration.path[0].name == format!("$derived_{}", kind.name())
            && self.implicit_callable_signature(required, cancel)?.as_ref()
                == Some(&selected.signature))
    }
}
