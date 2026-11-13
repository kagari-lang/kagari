//! Immutable native application facts are program edges, retained by active windows.
#[cfg(test)]
mod tests;

#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{
        MetadataEdge,
        applications::{AdmittedApplication, ApplicationRecord},
        managed::sources::MetadataSource,
    },
    frame::types::TypeEnvironment,
    module::ExecutableModule,
    native::binding::LinkedNativeFunction,
};
use kagari_bytecode::instruction::NativeImportId;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct NativeApplication {
    pub(crate) owner: ExecutableModule,
    pub(crate) environment: TypeEnvironment,
    pub(crate) function: LinkedNativeFunction,
}

impl NativeApplication {
    pub(crate) fn trace<'a, E, F>(&'a self, visit: &mut F) -> Result<(), E>
    where
        F: FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized,
    {
        visit(MetadataEdge::Program(&self.owner))?;
        visit(MetadataEdge::EnvironmentFacts(&self.environment))?;
        for operation in &self.function.selected {
            if let Some(callable) = operation.ready() {
                callable.trace(&self.owner, visit)?;
            }
        }
        Ok(())
    }
}

impl Runtime {
    pub(crate) fn prepare_native_application(
        &mut self,
        execution: SessionId,
        owner: &ExecutableModule,
        import: NativeImportId,
        environment: TypeEnvironment,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<AdmittedApplication, RuntimeError> {
        self.validate_executable_module(owner)?;
        self.validate_environment_in(protected, &environment)?;
        if let Some(application) = self
            .modules
            .native_application(owner, import, environment.id)
            && self.native_application_is_protected(protected, &application)?
        {
            return self.alloc_application(
                execution,
                ApplicationRecord::Native(application),
                protected,
            );
        }
        let body = owner
            .bytecode
            .native_imports
            .get(import.index())
            .and_then(|import| import.generic.as_ref())
            .ok_or_else(|| RuntimeError::module_validation("native application template"))?;
        if !environment.types.matches(body) {
            return Err(RuntimeError::module_validation(
                "native application type scope",
            ));
        }
        let function = self
            .modules
            .native_binding(owner, import)
            .ok_or_else(|| RuntimeError::module_validation("unlinked native application"))?;
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::NativePreparation);
        // The native import owns its checked selections. Caller bounds supply
        // Forward leaves; shared receiver/structural selections bind here using
        // the native entry's actual type scope.
        let operations = self.bind_operations_in(
            execution,
            owner,
            Some(environment.clone()),
            &owner.bytecode.native_imports[import.index()].callables,
            protected,
        )?;
        let prepared = Arc::new(NativeApplication {
            owner: owner.clone(),
            function: function.apply(
                self,
                execution,
                owner,
                environment.clone(),
                &operations,
                protected,
            )?,
            environment,
        });
        self.publish_native_application(owner, import, prepared.clone(), protected)?;
        self.alloc_application(execution, ApplicationRecord::Native(prepared), protected)
    }
}
