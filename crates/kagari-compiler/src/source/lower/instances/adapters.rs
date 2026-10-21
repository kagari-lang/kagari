//! Compose tuple operation witnesses using the shared structural adapter body.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::span::Span;
use kagari_contract::callable::shared::SharedCall;
use kagari_hir::types::semantic::{raise_nominal_type, raise_type};
use kagari_mir::ids::InstanceId;
use kagari_types::{
    callable::CallableImplementation, declaration::requirement::NativeCallableRequirement,
    language::adapter::adapter_contract, ty::Ty,
};

impl InstancePlanner<'_> {
    pub(crate) fn tuple_protocol_call(
        &mut self,
        function: InstanceId,
        required: NativeCallableRequirement,
        span: Span,
    ) -> Result<SharedCall, MirLoweringError> {
        let definitions = self.definitions();
        let (_, signature) = adapter_contract(&required, &definitions)
            .ok_or(MirLoweringError::MissingBinding("tuple protocol signature"))?;
        let Ty::Tuple(arguments) = &required.receiver else {
            return Err(MirLoweringError::MissingBinding("tuple protocol receiver"));
        };
        let mut requirements = Vec::new();
        for ty in arguments {
            let operation = NativeCallableRequirement {
                receiver: ty.clone(),
                ..required.clone()
            };
            if !requirements.contains(&operation) {
                requirements.push(operation);
            }
        }
        let construction_requirements = requirements
            .iter()
            .map(|required| {
                self.construction_requirement(
                    &raise_type(&required.receiver),
                    &raise_nominal_type(&required.interface),
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let operations = self.bind_operations(requirements, span)?;
        let constructions = self.bind_constructions(construction_requirements, span)?;
        let instance = self.instances[function.index()].lower(
            self.catalog,
            self.options,
            &definitions,
            span,
        )?;
        Ok(SharedCall {
            constructions,
            receiver_access: None,
            instance,
            implementation: CallableImplementation::Script,
            arguments: arguments.clone(),
            signature,
            operations,
        })
    }
}
