//! Resolve a selected operation's type mapping once before a native algorithm runs.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        BoundOperation, TypeEnvironment,
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
use std::{rc::Rc, slice};

impl LinkedCallable {
    pub(crate) fn prepare(
        runtime: &Runtime,
        caller: &LoadedModule,
        caller_environment: &Rc<TypeEnvironment>,
        required: &NativeCallableRequirement<DefinitionId>,
        operation: &BoundOperation,
    ) -> NativeResult<Self> {
        let (binders, environment) = if let Some(generic) = &operation.generic {
            let arguments = runtime.type_arguments(
                caller,
                Some(caller_environment.clone()),
                &required.arguments,
            )?;
            let mut binders = TypeEnvironment::new(
                runtime.definition_context(),
                generic.parameters.clone(),
                arguments,
            )?;
            binders.include(generic.receiver_environment.clone())?;
            let binders = Rc::new(binders);
            let environment = if generic.entry_parameters.is_empty() {
                None
            } else {
                let mut environment = TypeEnvironment::new(
                    runtime.definition_context(),
                    generic.entry_parameters.clone(),
                    runtime.type_arguments(
                        &operation.owner,
                        Some(binders.clone()),
                        &generic.entry_arguments,
                    )?,
                )?;
                if let Some(group) = runtime.bind_receiver_operations(
                    &operation.owner,
                    operation.target,
                    &generic.receiver_table,
                    operation.receiver_operations.upgrade(),
                )? {
                    environment.operations.receiver(group);
                }
                environment
                    .operations
                    .extend(caller_environment.operations.clone());
                Some(Rc::new(environment))
            };
            (Some(binders), environment)
        } else {
            (None, None)
        };
        let params = runtime.type_arguments(
            &operation.owner,
            binders.clone(),
            &operation.signature.params,
        )?;
        let result = runtime
            .type_arguments(
                &operation.owner,
                binders,
                slice::from_ref(&operation.signature.result),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("selected operation result"))?;
        Ok(Self {
            owner: CallableOwner::Pinned(operation.owner.clone(), operation.retention.clone()),
            target: operation.target,
            params: params
                .iter()
                .map(|argument| argument.ty().clone())
                .collect(),
            result: result.ty().clone(),
            primitive: operation.primitive,
            environment,
            scoped_signature: (result.has_origin() || params.iter().any(TypeArgument::has_origin))
                .then(|| Rc::new(ScopedSignature { params, result })),
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
                .is_some_and(|ty| runtime.matches_interface_method_abi(value, ty, owner)),
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
            None => runtime.matches_interface_method_abi(value, &self.result, owner),
        }
    }
}
