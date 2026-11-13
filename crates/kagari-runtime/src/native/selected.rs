//! Resolve a selected operation's type mapping once before a native algorithm runs.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{
        applications::ApplicationRecord, environments::EnvironmentSnapshot, groups::OperationId,
        managed::sources::MetadataSource,
    },
    frame::types::{
        EnvironmentRecord, arguments::ScopedSignature, construction::ConstructionScope,
    },
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable, ScriptCall, ScriptInvoker, SelectedCall},
        conversion::{arguments::ArgumentValues, context::ConversionContext},
    },
    value::view::ValueView,
};
use kagari_types::declaration::requirement::NativeCallableRequirement;
use std::{
    slice,
    sync::{Arc, OnceLock},
};

impl LinkedCallable {
    pub(crate) fn prepare(
        runtime: &mut Runtime,
        execution: SessionId,
        caller: &ExecutableModule,
        caller_environment: &EnvironmentSnapshot,
        required: &NativeCallableRequirement,
        id: OperationId,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> NativeResult<Self> {
        let dictionary = caller_environment.groups.get(&id.group).ok_or_else(|| {
            RuntimeError::module_validation("native selected operation dictionary")
        })?;
        let operation = dictionary
            .get(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid native selected operation"))?;
        if let Some(id) = operation.application {
            let record = runtime.application_record_in(protected, id)?;
            let ApplicationRecord::Semantic(application) = record.as_ref() else {
                return Err(RuntimeError::module_validation(
                    "selected shared operation application kind",
                ));
            };
            let scoped = match &application.scoped_signature {
                Some(signature) => Arc::new(signature.clone()),
                None => Arc::new(ScopedSignature {
                    params: runtime.type_arguments(
                        &operation.owner,
                        None,
                        &application.signature.params,
                    )?,
                    result: runtime
                        .type_arguments(
                            &operation.owner,
                            None,
                            slice::from_ref(&application.signature.result),
                        )?
                        .pop()
                        .ok_or_else(|| {
                            RuntimeError::module_validation("selected application result scope")
                        })?,
                }),
            };
            return Ok(Self {
                owner: CallableOwner::Resolved(operation.owner.clone()),
                target: operation.target,
                params: application.signature.params.clone().into(),
                result: application.signature.result.clone(),
                primitive: operation.primitive,
                environment: application.environment.clone(),
                scoped_signature: OnceLock::from(scoped),
            });
        }
        let (binders, environment) = if let Some(generic) = &operation.generic {
            let arguments = runtime.type_arguments(
                caller,
                Some(caller_environment.record.types.clone()),
                &required.arguments,
            )?;
            let mut binders = EnvironmentRecord::new(
                operation.owner.definitions(),
                generic.parameters.clone(),
                arguments,
            )?;
            let receiver = generic
                .receiver_environment
                .as_ref()
                .map(|environment| runtime.environment_snapshot_in(protected, environment))
                .transpose()?;
            let mut constructions = ConstructionScope::new(caller);
            constructions.extend(&caller_environment.record.constructions);
            constructions.extend(&ConstructionScope::new(&operation.owner));
            if let Some(receiver) = &receiver {
                constructions.extend(&receiver.record.constructions);
            }
            binders.set_constructions(constructions.clone());
            binders.include(generic.receiver_environment.clone())?;
            protected
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("native operation sources"))?;
            let binders = runtime.alloc_environment(execution, binders, protected)?;
            protected.push(MetadataSource::Environment(binders.source()));
            let environment = if generic.entry_parameters.is_empty() {
                None
            } else {
                let mut environment = EnvironmentRecord::new(
                    operation.owner.definitions(),
                    generic.entry_parameters.clone(),
                    runtime.type_arguments(
                        &operation.owner,
                        Some(binders.facts().types.clone()),
                        &generic.entry_arguments,
                    )?,
                )?;
                environment.set_constructions(constructions);
                if let Some(receiver) = &receiver {
                    environment.extend_operations(receiver.record.operations().clone());
                }
                if Runtime::receiver_operations_required(&operation.owner, operation.target) {
                    environment.add_receiver(dictionary)?;
                }
                environment.extend_operations(caller_environment.record.operations().clone());
                protected
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("native operation sources"))?;
                let environment = runtime.alloc_environment(execution, environment, protected)?;
                protected.push(MetadataSource::Environment(environment.source()));
                Some(environment.facts().clone())
            };
            (Some(binders), environment)
        } else {
            (None, None)
        };
        let params = runtime.type_arguments(
            &operation.owner,
            binders
                .as_ref()
                .map(|environment| environment.facts().types.clone()),
            &operation.signature.params,
        )?;
        let result = runtime
            .type_arguments(
                &operation.owner,
                binders.map(|environment| environment.facts().types.clone()),
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
            // Even origin-free native types contain admitted declaration IDs.
            // Preserve the prepared signature rather than treating its closed
            // spelling as a new product-local expression at the host boundary.
            scoped_signature: OnceLock::from(Arc::new(ScopedSignature { params, result })),
        })
    }

    /// Execute an already selected relationship using caller-protected inputs.
    /// The conversion owns the destination before callee scratch is admitted.
    pub(in crate::native) fn call_source<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        invoke: ScriptInvoker,
        values: ArgumentValues<'_>,
    ) -> NativeResult<ValueView<'call>> {
        let owner = self.owner(cx.owner())?;
        cx.runtime().validate_executable_module(&owner)?;
        if cx.owner().program_identity() != owner.program_identity()
            || cx.owner().slot() != owner.slot()
        {
            return Err(RuntimeError::module_validation("selected conversion owner"));
        }
        let signature = self.scoped_signature.get().ok_or_else(|| {
            RuntimeError::module_validation("selected call signature is not prepared")
        })?;
        cx.validate_arguments(values)?;
        if values.len() != signature.params.len() {
            return Err(RuntimeError::module_validation("native callback arguments"));
        }
        for (index, expected) in signature.params.iter().enumerate() {
            let value = cx.argument_value(values, index)?;
            if !cx.matches_value(expected, &value)? {
                return Err(RuntimeError::module_validation("native callback arguments"));
            }
        }
        let (location, destination) = cx.result_place()?;
        let published = match self.primitive {
            Some(primitive) => cx
                .runtime_mut()
                .invoke_standard_builtin(&owner, primitive, values, destination)
                .map_err(|error| error.into_runtime_error())?,
            None => {
                let selected =
                    SelectedCall::new(&owner, self, cx.owner_origin(), cx.owner_metadata());
                invoke(
                    cx.runtime_mut(),
                    &owner,
                    ScriptCall::Selected(&selected),
                    values,
                    destination,
                )?
            }
        };
        let value = cx.published_result(published, location)?;
        if !cx.matches_value(&signature.result, &value)? {
            return Err(RuntimeError::module_validation("native callback result"));
        }
        Ok(value)
    }
}
