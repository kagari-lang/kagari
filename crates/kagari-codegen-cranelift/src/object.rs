//! Relocatable output using the same scalar lowering as JIT.
use crate::{CraneliftBackend, emit, internal_error};
use cranelift_module::default_libcall_names;
use cranelift_object::{ObjectBuilder, ObjectModule};
use kagari_codegen::{
    BackendConfiguration, BackendFunctionInput,
    diagnostic::BackendCompileError,
    object::{NativeObjectProduct, ObjectBackend},
};

// SAFETY: shared lowering preserves checked scalar semantics and runtime polling.
// ObjectModule emits relocations; helper addresses are supplied only at invocation.
unsafe impl ObjectBackend for CraneliftBackend {
    fn configuration(&self) -> BackendConfiguration {
        self.configuration.clone()
    }

    fn emit_object(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<NativeObjectProduct, BackendCompileError> {
        let builder = ObjectBuilder::new(self.isa.clone(), "kagari", default_libcall_names())
            .map_err(|error| internal_error(error.to_string()))?;
        let mut module = ObjectModule::new(builder);
        let (_, artifact) = emit::lower(&mut module, &self.configuration, input)?;
        let bytes = module
            .finish()
            .emit()
            .map_err(|error| internal_error(error.to_string()))?;
        Ok(NativeObjectProduct { artifact, bytes })
    }
}
