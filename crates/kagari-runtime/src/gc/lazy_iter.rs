//! Native iterator captures are GC-owned, typed, and pinned to their checked import.
use crate::{
    LoadedModule, RuntimeError,
    gc::{
        GcHeap, HeapObject,
        iter::{IteratorKind, NativeIter},
    },
    module::RetainedRuntimeProgram,
    value::Value,
};
use kagari_abi::types::AbiType;
use kagari_bytecode::EngineImportId;
use std::{cell::Cell, rc::Rc};

#[derive(Debug)]
pub(super) struct IteratorCapture {
    pub(super) captures: Vec<Value>,
    implementation: LoadedModule,
    import: EngineImportId,
    _retention: RetainedRuntimeProgram,
}

pub(crate) struct IteratorRequest {
    pub(crate) implementation: LoadedModule,
    pub(crate) import: EngineImportId,
    pub(crate) iterator: Value,
    pub(crate) captures: Vec<Value>,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native lazy iterator state mismatch")
}
impl GcHeap {
    pub(crate) fn alloc_iterator_capture(
        &self,
        owner: &LoadedModule,
        import: EngineImportId,
        captures: Vec<Value>,
        retention: RetainedRuntimeProgram,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        if captures.iter().any(|value| !self.validate_value(value)) {
            return Err(invalid());
        }
        self.alloc_object(HeapObject::IteratorCapture(Box::new(IteratorCapture {
            captures,
            implementation: owner.clone(),
            import,
            _retention: retention,
        })))
        .map(Value::GcHandle)
    }
    pub(crate) fn new_lazy_iter(
        &self,
        source: Value,
        dependencies: Vec<Value>,
        item_type: AbiType,
        owner: &LoadedModule,
        retention: RetainedRuntimeProgram,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let Value::GcHandle(capture) = source else {
            return Err(invalid());
        };
        if !matches!(
            self.readable_object(&self.objects.borrow(), capture),
            Some(HeapObject::IteratorCapture(_))
        ) {
            return Err(invalid());
        }
        let session = self.resources.active_session().ok_or_else(invalid)?;
        self.alloc_object(HeapObject::Iter(Box::new(NativeIter {
            kind: IteratorKind::Adapter { dependencies },
            source,
            item_type,
            position: 0,
            string: None,
            revision: 0,
            guard: None,
            loops: Rc::new(Cell::new(0)),
            session: Rc::downgrade(&session),
            owner: owner.clone(),
            _retention: retention,
        })))
        .map(Value::GcHandle)
    }
    pub(crate) fn iterator_request(
        &self,
        value: &Value,
        ty: &AbiType,
    ) -> Result<Option<IteratorRequest>, RuntimeError> {
        self.validate_iter(value, ty)?;
        let Value::GcHandle(id) = value else {
            return Err(invalid());
        };
        let objects = self.objects.borrow();
        let Some(HeapObject::Iter(iter)) = self.readable_object(&objects, *id) else {
            return Err(invalid());
        };
        if matches!(iter.kind, IteratorKind::Collection) {
            return Ok(None);
        }
        let Value::GcHandle(capture) = &iter.source else {
            return Err(invalid());
        };
        let Some(HeapObject::IteratorCapture(state)) = self.readable_object(&objects, *capture)
        else {
            return Err(invalid());
        };
        Ok(Some(IteratorRequest {
            implementation: state.implementation.clone(),
            import: state.import,
            captures: state.captures.clone(),
            iterator: value.clone(),
        }))
    }
}
