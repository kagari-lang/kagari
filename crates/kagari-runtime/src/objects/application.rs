//! Applied call facts belong to the linked program, independently of receiver values.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::{
        application_key::{ApplicationArguments, ApplicationKey},
        applications::MethodApplication,
        groups::OperationGroupId,
    },
    frame::types::{
        EnvironmentRecord,
        arguments::{ScopedSignature, TypeArgument},
        construction::ConstructionScope,
        operations::OperationBindings,
    },
    objects::invocation::MethodInvocation,
};
use kagari_types::callable::Signature;
use std::slice;

impl Runtime {
    pub(super) fn apply_interface_method(
        &self,
        mut method: RootedInterfaceMethod,
        arguments: &[TypeArgument],
        operations: OperationBindings,
        mut constructions: ConstructionScope,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let view = method.view(self)?;
        if arguments.len() != view.type_parameters().len() {
            return Err(RuntimeError::module_validation(
                "interface method type arguments",
            ));
        }
        for argument in arguments {
            argument.validate(self)?;
        }
        constructions.extend(&ConstructionScope::new(view.proof_implementation()));
        let arguments = ApplicationArguments::new(
            view.proof_implementation(),
            arguments.to_vec(),
            constructions,
        )?;
        drop(view);
        method.invocation =
            self.apply_method_invocation(method.invocation, &arguments, operations)?;
        method.refresh_roots(self)?;
        Ok(method)
    }

    pub(crate) fn apply_method_invocation(
        &self,
        mut method: MethodInvocation,
        arguments: &ApplicationArguments,
        operations: OperationBindings,
    ) -> Result<MethodInvocation, RuntimeError> {
        let view = method.view(self)?;
        if arguments.values().len() != view.type_parameters().len() {
            return Err(RuntimeError::module_validation(
                "interface method type arguments",
            ));
        }
        for argument in arguments.values() {
            argument.validate(self)?;
        }
        if view.type_parameters().is_empty()
            && view.entry_parameters().is_empty()
            && view.receiver_environment().is_none()
            && operations.is_empty()
        {
            drop(view);
            return Ok(method);
        }
        let owner = view.proof_implementation().clone();
        let identity = view.identity();
        drop(view);
        let receiver_operations = method.receiver_operations(self)?;
        let key = ApplicationKey::new(identity, arguments, receiver_operations, &operations);
        let application = if let Some(prepared) = self.modules.method_application(&owner, &key) {
            prepared
        } else {
            let prepared = self.prepare_method_application(
                &method,
                arguments,
                operations,
                receiver_operations,
            )?;
            let prepared = self.gc.alloc_method_application(prepared)?;
            self.publish_method_application(&owner, key, prepared)?;
            prepared
        };
        method.application = Some(application);
        Ok(method)
    }

    fn prepare_method_application(
        &self,
        method: &MethodInvocation,
        arguments: &ApplicationArguments,
        operations: OperationBindings,
        receiver_operations: Option<OperationGroupId>,
    ) -> Result<MethodApplication, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::MethodPreparation);
        let view = method.view(self)?;
        let dependencies = view.receiver_environment();
        let mut supplied = dependencies
            .map(|environment| {
                self.gc
                    .environment(environment.id)
                    .map(|record| record.operations().clone())
                    .ok_or_else(|| {
                        RuntimeError::module_validation("receiver operation environment")
                    })
            })
            .transpose()?
            .unwrap_or_default();
        supplied.extend(operations);
        let operations = supplied;
        let mut binders = EnvironmentRecord::new(
            self.definition_context(),
            view.type_parameters().to_vec(),
            arguments.values().to_vec(),
        )?;
        let mut constructions =
            self.construction_scope(view.proof_implementation(), view.receiver_environment())?;
        constructions.extend(arguments.constructions());
        binders.set_constructions(constructions.clone());
        binders.include(view.receiver_environment().cloned())?;
        if let Some(group) = receiver_operations {
            binders.add_receiver(&self.gc, group)?;
        }
        // Associated results need selected output facts while the signature is
        // resolved, before publishing the executable application.
        binders.extend_operations(operations.clone());
        let binders = Some(self.alloc_environment(binders)?);
        let result_adapter = view.result_adapter().map(|adapter| {
            let mut adapter = adapter.clone();
            adapter.environment = binders.clone();
            adapter
        });
        let params = self.type_arguments(
            view.proof_implementation(),
            binders
                .as_ref()
                .map(|environment| environment.types.clone()),
            view.parameter_types(),
        )?;
        let result = self
            .type_arguments(
                view.proof_implementation(),
                binders
                    .as_ref()
                    .map(|environment| environment.types.clone()),
                slice::from_ref(view.return_type()),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("interface return type"))?;
        let signature = Signature {
            params: params
                .iter()
                .map(|argument| argument.ty().clone())
                .collect(),
            result: result.ty().clone(),
        };
        let scoped_signature = (result.has_origin() || params.iter().any(TypeArgument::has_origin))
            .then_some(ScopedSignature { params, result });
        let environment = if view.entry_parameters().is_empty() {
            None
        } else {
            let mut environment = EnvironmentRecord::new(
                self.definition_context(),
                view.entry_parameters().to_vec(),
                self.type_arguments(
                    view.proof_implementation(),
                    binders.map(|environment| environment.types.clone()),
                    view.entry_arguments(),
                )?,
            )?;
            environment.set_constructions(constructions);
            if let Some(group) = receiver_operations {
                environment.add_receiver(&self.gc, group)?;
            }
            environment.extend_operations(operations);
            Some(self.alloc_environment(environment)?)
        };
        Ok(MethodApplication {
            signature,
            scoped_signature,
            environment,
            result_adapter,
        })
    }
}

#[cfg(test)]
mod tests;
