//! Reuse closed method preparation; method-local arguments remain call-specific.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    frame::types::{
        TypeEnvironment,
        arguments::{ScopedSignature, TypeArgument},
    },
    gc::interfaces::MethodApplication,
};
use kagari_types::callable::Signature;
use std::{rc::Rc, slice};

impl Runtime {
    pub(super) fn apply_interface_method(
        &self,
        mut method: RootedInterfaceMethod,
        arguments: &[TypeArgument],
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        if arguments.len() != method.type_parameters().len() {
            return Err(RuntimeError::module_validation(
                "interface method type arguments",
            ));
        }
        for argument in arguments {
            argument.validate(self)?;
        }
        if method.type_parameters().is_empty()
            && method.entry_parameters().is_empty()
            && method.receiver_environment().is_none()
        {
            return Ok(method);
        }
        let reusable = method.type_parameters().is_empty();
        let application = if reusable && let Some(prepared) = method.application_cell().get() {
            prepared.clone()
        } else {
            let prepared = Rc::new(self.prepare_method_application(&method, arguments)?);
            if reusable {
                method
                    .application_cell()
                    .set(prepared.clone())
                    .expect("method application prepared once");
            }
            prepared
        };
        method.environment = application.environment.clone();
        // A descriptor's cache contains only types. Keeping its supplying group
        // here would create group -> descriptor -> application -> group cycles.
        if method.is_operation()
            && method.environment.is_some()
            && let Some(group) = method.receiver_operations(self)?
            && let Some(environment) = &mut method.environment
        {
            Rc::make_mut(environment).operations.receiver(group);
        }
        method.application = Some(application);
        Ok(method)
    }

    fn prepare_method_application(
        &self,
        method: &RootedInterfaceMethod,
        arguments: &[TypeArgument],
    ) -> Result<MethodApplication, RuntimeError> {
        let mut binders = TypeEnvironment::new(
            self.definition_context(),
            method.type_parameters().to_vec(),
            arguments.to_vec(),
        )?;
        binders.include(method.receiver_environment().cloned())?;
        let binders = Some(Rc::new(binders));
        let result_adapter = method.result_adapter().map(|adapter| {
            let mut adapter = adapter.clone();
            adapter.environment = binders.clone();
            adapter
        });
        let params = self.type_arguments(
            method.implementation(),
            binders.clone(),
            method.parameter_types(),
        )?;
        let result = self
            .type_arguments(
                method.implementation(),
                binders.clone(),
                slice::from_ref(method.return_type()),
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
        let environment = if method.entry_parameters().is_empty() {
            None
        } else {
            let mut environment = TypeEnvironment::new(
                self.definition_context(),
                method.entry_parameters().to_vec(),
                self.type_arguments(method.implementation(), binders, method.entry_arguments())?,
            )?;
            if !method.is_operation()
                && let Some(group) = method.receiver_operations(self)?
            {
                environment.operations.receiver(group);
            }
            Some(Rc::new(environment))
        };
        Ok(MethodApplication {
            signature,
            scoped_signature,
            environment,
            result_adapter,
        })
    }
}
