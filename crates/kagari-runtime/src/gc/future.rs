//! Claiming removes cold captures without lending a heap borrow to submission.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    gc::{
        GcHeap, HeapObjectId, collector,
        storage::{HeapObject, append_value_edges},
    },
    module::LoadedModule,
    native::{
        binding::NativeResult,
        future::{ColdFuture, FuturePayload},
    },
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;

impl GcHeap {
    /// Check actual retained edges, including mutable cells and erased receivers.
    /// Tracing uses the collection guard so native hooks cannot mutate or reenter.
    pub(crate) fn validate_async_values(&self, values: &[Value]) -> NativeResult<()> {
        self.ensure_no_native_borrow()?;
        let valid = self.resources.collect_operation(|| {
            let valid_edges = |edges: &[&Value]| {
                edges
                    .iter()
                    .all(|value| value.is_storable() && self.validate_value(value))
            };
            let mut edges = values.iter().rev().collect::<Vec<_>>();
            if !valid_edges(&edges) {
                return None;
            }
            let objects = self.objects.try_borrow().ok()?;
            let interfaces = self.interfaces.try_borrow().ok()?;
            let mut pending = Vec::new();
            append_value_edges(&mut edges, &mut pending);
            collector::mark(pending, |id, pending| {
                self.readable_object(&objects, id)?
                    .trace(&interfaces, &mut |value| edges.push(value))?;
                if !valid_edges(&edges) {
                    return None;
                }
                append_value_edges(&mut edges, pending);
                Some(())
            })
        })?;
        if valid.is_none() {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "async state retains an invalid or scoped value",
            ));
        }
        Ok(())
    }

    pub(crate) fn future_contract(
        &self,
        id: HeapObjectId,
    ) -> NativeResult<(LoadedModule, Ty<DefinitionId>, Option<TypeArgument>)> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting Future storage borrow"))?;
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else {
            return Err(RuntimeError::module_validation("invalid Future receiver"));
        };
        object.payload::<FuturePayload>()?;
        Ok((
            object.owner().clone(),
            object.ty.clone(),
            object.scope.clone(),
        ))
    }

    pub(crate) fn take_cold_future(&self, id: HeapObjectId) -> NativeResult<ColdFuture> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        let cold = {
            let mut objects = self.objects_mut()?;
            let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
                return Err(RuntimeError::module_validation("invalid Future receiver"));
            };
            object
                .payload_mut::<FuturePayload>()?
                .cold
                .take()
                .ok_or_else(|| {
                    RuntimeError::new(
                        RuntimeErrorKind::ScriptTrap,
                        "Future has already been awaited",
                    )
                })?
        };
        self.release_heap_units(cold.values().len());
        Ok(cold)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Runtime,
        gc::storage::HeapObject,
        value::{EphemeralValue, EphemeralValueId, Value},
    };
    use kagari_abi::representation::ValueType;
    use std::slice;

    #[test]
    fn async_retention_graph_contract() {
        let runtime = Runtime::default();
        let heap = runtime.gc();
        let cell = heap.alloc_cell(ValueType::Unit, Value::Unit).unwrap();
        let root = Value::Cell(cell);
        heap.validate_async_values(slice::from_ref(&root)).unwrap();
        let replace = |value| {
            let mut objects = heap.objects.borrow_mut();
            let Some(HeapObject::Cell { value: stored, .. }) = heap.object_mut(&mut objects, cell)
            else {
                unreachable!();
            };
            *stored = value;
        };
        // Model a cyclic graph and a later invalid hidden edge without weakening
        // ordinary mutation admission, which already rejects scoped values.
        replace(Value::Tuple(vec![root.clone()]));
        heap.validate_async_values(slice::from_ref(&root)).unwrap();
        replace(Value::Tuple(vec![Value::Ephemeral(
            EphemeralValue::Runtime(EphemeralValueId(1)),
        )]));
        assert!(heap.validate_async_values(slice::from_ref(&root)).is_err());
        replace(Value::Unit);
        heap.validate_async_values(slice::from_ref(&root)).unwrap();
        assert!(
            Runtime::default()
                .gc()
                .validate_async_values(slice::from_ref(&root))
                .is_err()
        );
        runtime.collect_garbage().unwrap();
        assert!(heap.validate_async_values(slice::from_ref(&root)).is_err());
        assert!(!runtime.is_quarantined());
    }
}
