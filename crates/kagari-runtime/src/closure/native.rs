use crate::{
    Runtime,
    closure::{ClosureTarget, ClosureValueSnapshot, NativeClosure},
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::types::arguments::ScopedSignature,
    module::LoadedModule,
    native::{binding::NativeResult, context::LinkedCallable},
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use std::sync::Arc;

impl Runtime {
    pub(crate) fn make_native_closure(
        &self,
        caller: &LoadedModule,
        target: &LinkedCallable,
        signature: Arc<ScopedSignature>,
    ) -> NativeResult<Value> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        let owner = target.owner(caller)?;
        self.validate_loaded_module(&owner)?;
        let CallableTarget::Native(import) = target.target else {
            return Err(RuntimeError::module_validation(
                "native closure requires a native entry",
            ));
        };
        if target.primitive.is_some() || owner.bytecode.native_imports.get(import.index()).is_none()
        {
            return Err(RuntimeError::module_validation("native closure import"));
        }
        let snapshot = ClosureValueSnapshot {
            environment: target.environment.clone(),
            implementation: owner,
            target: ClosureTarget::Native(NativeClosure { import, signature }),
            captures: Vec::new(),
        };
        // No LinkedCallable, ProgramLease or RootSet is embedded in the heap.
        // The ordinary closure program/environment edges retain this native entry.
        self.validate_metadata(MetadataEdge::Closure(&snapshot))?;
        self.gc.alloc_closure(snapshot).map(Value::Closure)
    }
}
