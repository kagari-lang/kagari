//! Immutable native application facts are program edges, retained by active windows.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime, error::RuntimeError, execution_metadata::MetadataEdge, frame::types::TypeEnvironment,
    module::LoadedModule, native::binding::LinkedNativeFunction,
};
use kagari_bytecode::instruction::NativeImportId;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct NativeApplication {
    pub(crate) owner: LoadedModule,
    pub(crate) environment: TypeEnvironment,
    pub(crate) function: LinkedNativeFunction,
}

impl NativeApplication {
    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.push(MetadataEdge::Program(&self.owner));
        pending.push(MetadataEdge::Environment(self.environment.id));
        for operation in &self.function.selected {
            if let Some(callable) = operation.ready() {
                callable.trace(&self.owner, pending);
            }
        }
    }
}

impl Runtime {
    pub(crate) fn prepare_native_application(
        &self,
        owner: &LoadedModule,
        import: NativeImportId,
        environment: TypeEnvironment,
    ) -> Result<Arc<NativeApplication>, RuntimeError> {
        self.validate_loaded_module(owner)?;
        if self.gc.environment(environment.id).is_none() {
            return Err(RuntimeError::module_validation(
                "invalid native application environment",
            ));
        }
        if let Some(application) = self
            .modules
            .native_application(owner, import, environment.id)
        {
            return Ok(application);
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
        let prepared = Arc::new(NativeApplication {
            owner: owner.clone(),
            function: function.apply(self, owner, environment.clone())?,
            environment,
        });
        self.publish_native_application(owner, import, prepared.clone())?;
        Ok(prepared)
    }
}
