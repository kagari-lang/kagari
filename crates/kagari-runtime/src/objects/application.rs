//! Reuse closed method preparation; method-local arguments remain call-specific.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::applications::MethodApplication,
    frame::types::{
        EnvironmentRecord,
        arguments::{ScopedSignature, TypeArgument},
        operations::OperationBindings,
    },
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
            method.refresh_roots(self)?;
            return Ok(method);
        }
        let reusable = view.type_parameters().is_empty() && operations.is_empty();
        let cached = view.cached_application();
        drop(view);
        let application = if reusable && let Some(prepared) = cached {
            prepared
        } else {
            let prepared = self.prepare_method_application(&method, arguments, operations)?;
            let prepared = self.gc.alloc_method_application(prepared)?;
            if reusable {
                self.cache_method_application(method.selection, prepared)?;
            }
            prepared
        };
        method.environment = self
            .gc
            .method_application(application)
            .ok_or_else(|| RuntimeError::module_validation("invalid cached method application"))?
            .environment
            .clone();
        method.application = Some(application);
        method.refresh_roots(self)?;
        Ok(method)
    }

    fn prepare_method_application(
        &self,
        method: &RootedInterfaceMethod,
        arguments: &[TypeArgument],
        operations: OperationBindings,
    ) -> Result<MethodApplication, RuntimeError> {
        let view = method.view(self)?;
        let mut binders = EnvironmentRecord::new(
            self.definition_context(),
            view.type_parameters().to_vec(),
            arguments.to_vec(),
        )?;
        binders.include(view.receiver_environment().cloned())?;
        let receiver_operations = method.receiver_operations(self)?;
        if let Some(group) = receiver_operations {
            binders.add_receiver(&self.gc, group)?;
        }
        // Associated results need selected output facts while the signature is
        // resolved, before publishing the executable application.
        binders.extend_operations(operations.clone());
        let binders = Some(self.gc.alloc_environment(binders)?);
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
            Some(self.gc.alloc_environment(environment)?)
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
