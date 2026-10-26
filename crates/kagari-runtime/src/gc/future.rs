//! Claiming removes cold captures without lending a heap borrow to submission.
#[cfg(test)]
use crate::gc::retention_probe::{self, Event};
use crate::{
    closure::CallableObservation,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    gc::{GcHeap, HeapObjectId, collector::Visited, roots::RootSet, storage::append_value_edges},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        future::{ColdFuture, FuturePayload},
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_types::ty::Ty;

/// Scratch for one guarded operation, never a reusable proof of object safety.
pub(crate) struct AsyncValues<'heap> {
    heap: &'heap GcHeap,
    visited: Visited<HeapObjectId>,
    edges: Vec<Value>,
    pending: Vec<HeapObjectId>,
}

impl AsyncValues<'_> {
    pub(crate) fn validate(&mut self, values: &[Value]) -> NativeResult<()> {
        #[cfg(test)]
        retention_probe::record(self.heap, Event::Roots(values.len()));
        let Self {
            heap,
            visited,
            edges,
            pending,
        } = self;
        let valid_edges = |values: &[Value]| {
            values
                .iter()
                .all(|value| value.is_storable(heap) && heap.validate_value(value))
        };
        let valid = (|| {
            edges.extend(values.iter().rev().copied());
            // Validate each representation, even if its object was already seen
            // through another root or a differently tagged incoming edge.
            if !valid_edges(edges) {
                return None;
            }
            let objects = heap.objects.try_borrow().ok()?;
            let interfaces = heap.interfaces.try_borrow().ok()?;
            let mut workspace = heap.native_scan_workspace.try_borrow_mut().ok()?;
            append_value_edges(edges, pending);
            visited.mark(pending, |id, pending| {
                #[cfg(test)]
                retention_probe::record(heap, Event::Object(id));
                heap.readable_object(&objects, id)?.trace(
                    &interfaces,
                    &mut workspace,
                    &mut |value| edges.push(value),
                )?;
                #[cfg(test)]
                retention_probe::record(heap, Event::Edges(id, edges.len()));
                if !valid_edges(edges) {
                    return None;
                }
                append_value_edges(edges, pending);
                Some(())
            })
        })();
        valid.ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "async state retains an invalid or scoped value",
            )
        })
    }
}

impl GcHeap {
    /// Check actual retained edges, including mutable cells and erased receivers.
    pub(crate) fn validate_async_values(&self, values: &[Value]) -> NativeResult<()> {
        self.with_async_values(|walk| walk.validate(values))
    }

    /// Share visited identities only inside this collection-guarded operation.
    /// The callback is runtime admission code, not a host callback. The guard
    /// rejects mutation/reentry from native trace hooks, including ignored errors.
    pub(crate) fn with_async_values<T>(
        &self,
        operation: impl FnOnce(&mut AsyncValues<'_>) -> NativeResult<T>,
    ) -> NativeResult<T> {
        self.ensure_no_native_borrow()?;
        #[cfg(test)]
        retention_probe::record(self, Event::Walk);
        self.resources.collect_operation(|| {
            operation(&mut AsyncValues {
                heap: self,
                visited: Visited::new(),
                edges: Vec::new(),
                pending: Vec::new(),
            })
        })?
    }

    pub(crate) fn future_contract(
        &self,
        id: HeapObjectId,
    ) -> NativeResult<(ExecutableModule, Ty, Option<TypeArgument>)> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting Future storage borrow"))?;
        let Some(object) = self.native_ref(&objects, id) else {
            return Err(RuntimeError::module_validation("invalid Future receiver"));
        };
        object.payload::<FuturePayload>()?;
        Ok((
            object.owner().clone(),
            object.ty.clone(),
            object.scope.clone(),
        ))
    }

    pub(crate) fn future_script(&self, id: HeapObjectId) -> NativeResult<Option<Value>> {
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("Future storage borrow"))?;
        let object = self
            .native_ref(&objects, id)
            .ok_or_else(|| RuntimeError::module_validation("invalid Future receiver"))?;
        Ok(match object.payload::<FuturePayload>()?.cold.as_ref() {
            Some(ColdFuture::Script(value)) => Some(*value),
            _ => None,
        })
    }

    /// Copy known producer identity without claiming, polling or rooting its state.
    pub(crate) fn future_native_origin(
        &self,
        id: HeapObjectId,
    ) -> NativeResult<Option<CallableObservation>> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("Future storage borrow"))?;
        let object = self
            .native_ref(&objects, id)
            .ok_or_else(|| RuntimeError::module_validation("invalid Future receiver"))?;
        Ok(match object.payload::<FuturePayload>()?.cold.as_ref() {
            Some(ColdFuture::Native(cold)) => cold.origin,
            _ => None,
        })
    }

    /// Reserve roots while both heap payloads still trace their cold captures.
    pub(crate) fn prepare_future_pair(&self, ids: [HeapObjectId; 2]) -> NativeResult<[RootSet; 2]> {
        if ids[0] == ids[1] {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "duplicate Future operands",
            ));
        }
        let captures = {
            let objects = self
                .objects
                .try_borrow()
                .map_err(|_| RuntimeError::module_validation("Future storage borrow"))?;
            let read = |id| -> NativeResult<Vec<Value>> {
                let object = self
                    .native_ref(&objects, id)
                    .ok_or_else(|| RuntimeError::module_validation("invalid Future receiver"))?;
                let cold = object
                    .payload::<FuturePayload>()?
                    .cold
                    .as_ref()
                    .ok_or_else(|| {
                        RuntimeError::new(
                            RuntimeErrorKind::ScriptTrap,
                            "Future has already been awaited",
                        )
                    })?;
                Ok(cold.values().to_vec())
            };
            [read(ids[0])?, read(ids[1])?]
        };
        let [left, right] = captures;
        let root = |values| {
            self.root_execution_values(values)
                .ok_or_else(|| RuntimeError::resource_limit("Future capture roots"))
        };
        Ok([root(left)?, root(right)?])
    }

    /// Recheck and claim both payloads under one borrow. No callback, allocation or
    /// collection occurs between claims; callers already own their capture roots.
    pub(crate) fn take_cold_future_pair(
        &self,
        ids: [HeapObjectId; 2],
    ) -> NativeResult<[ColdFuture; 2]> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        if ids[0] == ids[1] {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "duplicate Future operands",
            ));
        }
        let mut objects = self.objects_mut()?;
        for id in ids {
            let object = self
                .native_ref(&objects, id)
                .ok_or_else(|| RuntimeError::module_validation("invalid Future receiver"))?;
            if object.payload::<FuturePayload>()?.cold.is_none() {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "Future has already been awaited",
                ));
            }
        }
        Ok(ids.map(|id| {
            let object = self.native_mut(&mut objects, id).expect("validated Future");
            let cold = object
                .payload_mut::<FuturePayload>()
                .expect("validated Future payload")
                .cold
                .take()
                .expect("checked cold state");
            let remaining = object.storage_charge(HeapCharge::default());
            let released = HeapCharge::new(
                object.charge.logical_units - remaining.logical_units,
                object.charge.retained_bytes - remaining.retained_bytes,
            );
            object.charge = remaining;
            self.resources.release_heap_charge(released);
            cold
        }))
    }

    pub(crate) fn take_cold_future(&self, id: HeapObjectId) -> NativeResult<ColdFuture> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        let cold = {
            let mut objects = self.objects_mut()?;
            let Some(object) = self.native_mut(&mut objects, id) else {
                return Err(RuntimeError::module_validation("invalid Future receiver"));
            };
            let cold = object
                .payload_mut::<FuturePayload>()?
                .cold
                .take()
                .ok_or_else(|| {
                    RuntimeError::new(
                        RuntimeErrorKind::ScriptTrap,
                        "Future has already been awaited",
                    )
                })?;
            let remaining = object.storage_charge(HeapCharge::default());
            let released = HeapCharge::new(
                object.charge.logical_units - remaining.logical_units,
                object.charge.retained_bytes - remaining.retained_bytes,
            );
            object.charge = remaining;
            self.resources.release_heap_charge(released);
            cold
        };
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
        replace(heap.alloc_tuple(vec![root]).unwrap());
        heap.validate_async_values(slice::from_ref(&root)).unwrap();
        assert!(
            heap.with_async_values(|walk| {
                walk.validate(&[root])?;
                // The identity was visited, but this new root has an invalid tag.
                walk.validate(&[Value::Str(cell)])
            })
            .is_err()
        );
        replace(
            heap.alloc_tuple(vec![
                heap.alloc_ephemeral(EphemeralValue::Runtime(EphemeralValueId(1)))
                    .unwrap(),
            ])
            .unwrap(),
        );
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
