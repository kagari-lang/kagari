//! Immutable native application facts are program edges, retained by active windows.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{MetadataEdge, scan_records::NativeApplicationScanId},
    frame::types::TypeEnvironment,
    module::ExecutableModule,
    native::binding::LinkedNativeFunction,
};
use kagari_bytecode::instruction::NativeImportId;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct NativeApplication {
    pub(crate) scan_id: NativeApplicationScanId,
    pub(crate) owner: ExecutableModule,
    pub(crate) environment: TypeEnvironment,
    pub(crate) function: LinkedNativeFunction,
}

impl NativeApplication {
    pub(crate) fn scan_len(&self) -> usize {
        2 + self.function.selected.len() * 2
    }

    pub(crate) fn scan_field(&self, index: usize) -> Option<MetadataEdge<'_>> {
        match index {
            0 => Some(MetadataEdge::Program(&self.owner)),
            1 => Some(MetadataEdge::Environment(self.environment.id)),
            _ => self
                .function
                .selected
                .get((index - 2) / 2)?
                .ready()?
                .scan_field(&self.owner, (index - 2) % 2),
        }
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend((0..self.scan_len()).filter_map(|index| self.scan_field(index)));
    }
}

impl Runtime {
    pub(crate) fn prepare_native_application(
        &self,
        owner: &ExecutableModule,
        import: NativeImportId,
        environment: TypeEnvironment,
    ) -> Result<Arc<NativeApplication>, RuntimeError> {
        self.validate_executable_module(owner)?;
        if self.gc.environment(environment.id).is_none() {
            return Err(RuntimeError::module_validation(
                "invalid native application environment",
            ));
        }
        if let Some(application) =
            self.modules
                .native_application(&self.gc, owner, import, environment.id)
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
        // The native import owns its checked selections. Caller bounds supply
        // Forward leaves; shared receiver/structural selections bind here using
        // the native entry's actual type scope.
        let operations = self.bind_operations_in(
            owner,
            Some(environment.clone()),
            &owner.bytecode.native_imports[import.index()].callables,
        )?;
        let function = function.apply(self, owner, environment.clone(), &operations)?;
        let prepared = self
            .gc
            .admit_native_application(self.modules(), |scan_id| NativeApplication {
                scan_id,
                owner: owner.clone(),
                function,
                environment,
            })?;
        self.publish_native_application(owner, import, prepared.clone())?;
        Ok(prepared)
    }
}
