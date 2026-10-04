//! Exclusive sequence storage leases restore completed edits on every exit path.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObject, HeapObjectId, RootSet},
    native::{
        binding::NativeResult,
        sequence::{SequencePayload, SequenceStorage},
        sequence_edit::SequenceEdit,
    },
};
use std::mem;

struct StorageLease<'heap> {
    heap: &'heap GcHeap,
    id: HeapObjectId,
    values: SequenceStorage,
    before: usize,
    revision: u64,
    // Rust sorting temporarily moves elements out of the slice. These roots
    // protect reference values independently of those transient storage slots.
    _roots: Option<RootSet>,
}

impl Drop for StorageLease<'_> {
    fn drop(&mut self) {
        let restored = (|| {
            let mut objects = self.heap.objects.try_borrow_mut().ok()?;
            let HeapObject::Native(object) = self.heap.object_mut(&mut objects, self.id)? else {
                return None;
            };
            let sequence = object.payload_mut::<SequencePayload>().ok()?;
            mem::swap(&mut sequence.values, &mut self.values);
            sequence.leased_units = None;
            let after = sequence.values.len();
            objects[self.id.slot].revision = self.revision;
            self.heap.release_heap_units(self.before - after);
            Some(())
        })();
        self.heap.mutations.borrow_mut().remove(&self.id);
        if restored.is_none() {
            self.heap
                .resources
                .quarantine("sequence lease restoration failed");
        }
    }
}

impl GcHeap {
    // SequenceMutHandle retains the receiver's argument root. No heap borrow
    // spans the callback; only the receiver's detached slots are inaccessible.
    pub(crate) fn edit_sequence<R>(
        &self,
        id: HeapObjectId,
        edit: impl for<'buffer> FnOnce(SequenceEdit<'buffer>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_execution_allowed()?;
        self.ensure_no_native_borrow()?;
        self.ensure_structure_mutable(id)?;
        let references = self.with_native::<SequencePayload, _>(id, |sequence| {
            if sequence.values.traced().is_empty() {
                return Ok(Vec::new());
            }
            let mut values = Vec::new();
            values
                .try_reserve_exact(sequence.values.traced().len())
                .map_err(|_| self.resource_limit("sequence edit roots"))?;
            values.extend_from_slice(sequence.values.traced());
            Ok(values)
        })?;
        let roots = if references.is_empty() {
            None
        } else {
            Some(self.root_execution_values(references).ok_or_else(invalid)?)
        };
        self.mutations
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| self.resource_limit("sequence edit registry"))?;
        let mut objects = self.objects.try_borrow_mut().map_err(|_| invalid())?;
        let revision = objects
            .get(id.slot)
            .ok_or_else(invalid)?
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        let sequence = object.payload_mut::<SequencePayload>()?;
        let before = sequence.values.len();
        let values = mem::replace(
            &mut sequence.values,
            SequenceStorage::empty(&sequence.element),
        );
        sequence.leased_units = Some(before);
        drop(objects);
        self.mutations.borrow_mut().insert(id, 1);
        let mut lease = StorageLease {
            heap: self,
            id,
            values,
            before,
            revision,
            _roots: roots,
        };
        let result = edit(SequenceEdit {
            values: &mut lease.values,
        });
        drop(lease);
        result
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid sequence edit receiver")
}

#[cfg(test)]
mod tests {
    use crate::{Runtime, error::RuntimeError, layout_fixtures::allocation_owner, value::Value};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn direct_scalar_lease_keeps_the_buffer_and_completed_writes_on_error() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let id = runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                vec![Value::I32(1), Value::I32(2)],
            )
            .unwrap();
        let root = runtime.root_value(Value::Array(id)).unwrap();
        let heap = runtime.gc();
        let address = heap
            .with_sequence_mut::<i32, _>(id, |values| Ok(values.as_ptr() as usize))
            .unwrap();
        let error = heap.edit_sequence(id, |mut edit| {
            edit.with_slice_mut::<i32, _>(|values| {
                assert_eq!(values.as_ptr() as usize, address);
                values.swap(0, 1);
                Err::<(), _>(RuntimeError::module_validation("test failure"))
            })
        });
        assert!(error.is_err());
        assert_eq!(
            heap.array_snapshot(id).unwrap(),
            vec![Value::I32(2), Value::I32(1)]
        );
        heap.with_sequence_mut::<i32, _>(id, |values| {
            assert_eq!(values.as_ptr() as usize, address);
            Ok(())
        })
        .unwrap();
        drop(root);
    }

    #[test]
    fn removals_survive_failure_and_unwind_and_release_lease_accounting() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let id = runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                (0..5).map(Value::I32).collect(),
            )
            .unwrap();
        let root = runtime.root_value(Value::Array(id)).unwrap();
        let heap = runtime.gc();
        let before = heap.stats().current_heap_units;
        let mut calls = 0;
        assert!(
            heap.edit_sequence(id, |mut edit| edit.retain(|value| {
                calls += 1;
                if value == Value::I32(3) {
                    return Err(RuntimeError::module_validation("test failure"));
                }
                Ok(value != Value::I32(1))
            }))
            .is_err()
        );
        assert_eq!(calls, 4);
        assert_eq!(
            heap.array_snapshot(id).unwrap(),
            vec![Value::I32(0), Value::I32(2), Value::I32(3), Value::I32(4)]
        );
        assert_eq!(heap.stats().current_heap_units, before - 1);
        assert!(
            catch_unwind(AssertUnwindSafe(|| heap.edit_sequence::<()>(
                id,
                |mut edit| {
                    edit.reverse();
                    panic!("test unwind");
                }
            )))
            .is_err()
        );
        assert_eq!(
            heap.array_snapshot(id).unwrap(),
            vec![Value::I32(4), Value::I32(3), Value::I32(2), Value::I32(0)]
        );
        heap.array_push(id, Value::I32(9)).unwrap();
        assert_eq!(heap.stats().current_heap_units, before);
        drop(root);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    }
}
