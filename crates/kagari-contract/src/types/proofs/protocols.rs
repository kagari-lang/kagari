//! Validate generated protocol functions against carried declarations and bounds.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionId},
};
use {
    crate::{
        effects::EffectSet,
        native_import::{
            callables::{NativeCallableApplication, NativeCallableOrigin},
            protocol::adapter_arguments,
        },
        types::proofs::ProofCatalog,
    },
    kagari_types::{
        callable::{CallableImplementation, Signature},
        declaration::requirement::NativeCallableRequirement,
        language::adapter::adapter_contract,
        ty::substitution::{TypeSubstitution, TypeTransformError},
    },
};

impl ProofCatalog<'_> {
    pub fn implicit_callable_signature(
        &self,
        required: &NativeCallableRequirement<DefinitionId>,
        cancel: &CancellationToken,
    ) -> Result<Option<Signature<DefinitionId>>, TypeTransformError> {
        let Some((_, expected)) = adapter_contract(required, &self.definitions) else {
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
            self.definitions
                .resolve(required.member)
                .ok()
                .and_then(|view| view.last())
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
        for bound in substitution.apply_bounds(&method.bounds, &self.definitions, cancel)? {
            if !self.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(None);
            }
        }
        let normalize = |ty| self.normalize(&substitution.apply(ty, cancel)?, cancel);
        let applied = Signature {
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
        required: &NativeCallableRequirement<DefinitionId>,
        selected: &NativeCallableApplication<DefinitionId>,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        if selected.origin == NativeCallableOrigin::Implementation {
            return Ok(self.select_callable(required, cancel)?.as_ref() == Some(selected));
        }
        let Some((kind, _)) = adapter_contract(required, &self.definitions) else {
            return Ok(false);
        };
        let declaration = &selected.instance.declaration;
        Ok(selected.requirement == *required
            && selected.implementation == CallableImplementation::Script
            && selected.effects == EffectSet::native_call()
            && selected.instance.arguments == adapter_arguments(required, &self.definitions)
            && self.definitions.resolve(*declaration).is_ok_and(|view| {
                view.segments().count() == 1
                    && view.last().is_some_and(|part| {
                        part.kind == DefinitionKind::Function
                            && part.occurrence == 0
                            && part.name == format!("$derived_{}", kind.name())
                    })
            })
            && self.implicit_callable_signature(required, cancel)?.as_ref()
                == Some(&selected.signature))
    }
}
