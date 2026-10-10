//! Applied call facts belong to the linked program, independently of receiver values.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::{
        application_key::ApplicationKey, applications::MethodApplication, groups::OperationGroupId,
    },
    frame::types::{
        EnvironmentRecord,
        arguments::{ScopedSignature, TypeArgument},
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
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        method.view(self)?;
        method.invocation =
            self.apply_method_invocation(method.invocation, arguments, operations)?;
        method.refresh_roots(self)?;
        Ok(method)
    }

    pub(crate) fn apply_method_invocation(
        &self,
        mut method: MethodInvocation,
        arguments: &[TypeArgument],
        operations: OperationBindings,
    ) -> Result<MethodInvocation, RuntimeError> {
        let view = method.view(self)?;
        if arguments.len() != view.type_parameters().len() {
            return Err(RuntimeError::module_validation(
                "interface method type arguments",
            ));
        }
        for argument in arguments {
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
        let owner = view.implementation().clone();
        let identity = view.identity();
        drop(view);
        let receiver_operations = method.receiver_operations(self)?;
        let key = ApplicationKey::new(
            identity,
            &owner,
            arguments,
            receiver_operations,
            &operations,
        )?;
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
        arguments: &[TypeArgument],
        operations: OperationBindings,
        receiver_operations: Option<OperationGroupId>,
    ) -> Result<MethodApplication, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::MethodPreparation);
        let view = method.view(self)?;
        let mut binders = EnvironmentRecord::new(
            self.definition_context(),
            view.type_parameters().to_vec(),
            arguments.to_vec(),
        )?;
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
            view.implementation(),
            binders
                .as_ref()
                .map(|environment| environment.types.clone()),
            view.parameter_types(),
        )?;
        let result = self
            .type_arguments(
                view.implementation(),
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
                    view.implementation(),
                    binders.map(|environment| environment.types.clone()),
                    view.entry_arguments(),
                )?,
            )?;
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
