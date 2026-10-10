//! Resolve a selected operation's type mapping once before a native algorithm runs.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::groups::OperationId,
    frame::types::{
        EnvironmentRecord, TypeEnvironment,
        arguments::{ScopedSignature, TypeArgument},
    },
    module::LoadedModule,
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable},
    },
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::declaration::requirement::NativeCallableRequirement;
use std::{slice, sync::Arc};

impl LinkedCallable {
    pub(crate) fn prepare(
        runtime: &Runtime,
        caller: &LoadedModule,
        caller_environment: &TypeEnvironment,
        required: &NativeCallableRequirement<DefinitionId>,
        id: OperationId,
    ) -> NativeResult<Self> {
        let operation = runtime
            .gc
            .bound_operation(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid native selected operation"))?;
        let (binders, environment) = if let Some(generic) = &operation.generic {
            let arguments = runtime.type_arguments(
                caller,
                Some(caller_environment.types.clone()),
                &required.arguments,
            )?;
            let mut binders = EnvironmentRecord::new(
                runtime.definition_context(),
                generic.parameters.clone(),
                arguments,
            )?;
            binders.include(generic.receiver_environment.clone())?;
            let binders = runtime.alloc_environment(binders)?;
            let environment = if generic.entry_parameters.is_empty() {
                None
            } else {
                let mut environment = EnvironmentRecord::new(
                    runtime.definition_context(),
                    generic.entry_parameters.clone(),
                    runtime.type_arguments(
                        &operation.owner,
                        Some(binders.types.clone()),
                        &generic.entry_arguments,
                    )?,
                )?;
                if let Some(group) = runtime.bind_receiver_operations(
                    &operation.owner,
                    operation.target,
                    &generic.receiver_table,
                    Some(id.group),
                )? {
                    environment.add_receiver(&runtime.gc, group)?;
                }
                environment.extend_operations(
                    runtime
                        .gc
                        .environment(caller_environment.id)
                        .ok_or_else(|| {
                            RuntimeError::module_validation("invalid caller environment")
                        })?
                        .operations()
                        .clone(),
                );
                Some(runtime.alloc_environment(environment)?)
            };
            (Some(binders), environment)
        } else {
            (None, None)
        };
        let params = runtime.type_arguments(
            &operation.owner,
            binders
                .as_ref()
                .map(|environment| environment.types.clone()),
            &operation.signature.params,
        )?;
        let result = runtime
            .type_arguments(
                &operation.owner,
                binders.map(|environment| environment.types.clone()),
                slice::from_ref(&operation.signature.result),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("selected operation result"))?;
        Ok(Self {
            owner: CallableOwner::Resolved(operation.owner.clone()),
            target: operation.target,
            params: params
                .iter()
                .map(|argument| argument.ty().clone())
                .collect(),
            result: result.ty().clone(),
            primitive: operation.primitive,
            environment,
            scoped_signature: (result.has_origin() || params.iter().any(TypeArgument::has_origin))
                .then(|| Arc::new(ScopedSignature { params, result })),
        })
    }

    pub(crate) fn matches_argument(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        index: usize,
        value: &Value,
    ) -> bool {
        match &self.scoped_signature {
            Some(signature) => signature
                .params
                .get(index)
                .is_some_and(|argument| argument.matches(runtime, value, owner)),
            None => self
                .params
                .get(index)
                .is_some_and(|ty| runtime.matches_type_in(value, ty, owner, None)),
        }
    }

    pub(crate) fn matches_result(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        value: &Value,
    ) -> bool {
        match &self.scoped_signature {
            Some(signature) => signature.result.matches(runtime, value, owner),
            None => runtime.matches_type_in(value, &self.result, owner, None),
        }
    }
}
