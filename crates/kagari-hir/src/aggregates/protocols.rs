//! Checked implicit protocol applications, independent of executable planning.
use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    types::{
        NominalType, TypeId, TypeSubstitution,
        semantic::{lower_nominal_type, lower_type},
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    callable::Signature,
    declaration::requirement::NativeCallableRequirement,
    language::{Protocol, adapter::adapter_contract},
};

pub struct ImplicitProtocolApplication {
    pub kind: Protocol,
    pub requirement: NativeCallableRequirement,
    pub signature: Signature,
}

impl AggregateCatalog {
    pub fn implicit_protocol_application(
        &self,
        required: &NativeCallableRequirement,
        receiver: &TypeId,
        interface: &NominalType,
        cancel: &CancellationToken,
    ) -> Result<Option<ImplicitProtocolApplication>, ImplementationSearchError> {
        let Some((kind, expected)) = adapter_contract(required) else {
            return Ok(None);
        };
        if lower_type(receiver) != required.receiver
            || lower_nominal_type(interface) != required.interface
            || !self.intrinsic_implementation(interface, receiver, &Default::default())
            || self
                .concrete_interface_implementation(
                    interface,
                    receiver,
                    &Default::default(),
                    100_000,
                    64,
                    cancel,
                )?
                .is_some()
        {
            return Ok(None);
        }
        let Some(contract) = self.trait_(&interface.declaration) else {
            return Ok(None);
        };
        let Some(method) = self.trait_method(&required.member) else {
            return Ok(None);
        };
        if contract.generic_params.len() != interface.arguments.len()
            || method.generic_params.len() != contract.generic_params.len()
        {
            return Ok(None);
        }
        let mut substitution = TypeSubstitution::default();
        substitution.extend(
            contract
                .generic_params
                .iter()
                .cloned()
                .zip(interface.arguments.iter().cloned()),
        );
        substitution.insert_receiver(interface.declaration.clone(), receiver.clone());
        let normalize = |ty: &TypeId| {
            lower_type(
                &self.normalize_type(
                    &ty.instantiate(&substitution)
                        .with_associated_types(interface),
                ),
            )
        };
        let signature = Signature {
            params: method
                .params
                .iter()
                .map(|param| normalize(&param.ty))
                .collect(),
            result: normalize(&method.return_type),
        };
        Ok(
            (signature == expected).then(|| ImplicitProtocolApplication {
                kind,
                requirement: required.clone(),
                signature,
            }),
        )
    }
}
