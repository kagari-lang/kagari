//! Atomic publication of a prepared permutation or primitive buffer edit.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObject, HeapObjectId},
    native::{binding::NativeResult, sequence::SequencePayload, sequence_edit::SequenceEdit},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

struct MutationLease {
    active: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
    id: HeapObjectId,
}
impl Drop for MutationLease {
    fn drop(&mut self) {
        self.active.borrow_mut().remove(&self.id);
    }
}
impl GcHeap {
    // The public SequenceMutHandle holds the existing native argument root.
    pub(crate) fn edit_sequence<R>(
        &self,
        id: HeapObjectId,
        edit: impl for<'buffer> FnOnce(SequenceEdit<'buffer>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_execution_allowed()?;
        self.ensure_no_native_borrow()?;
        self.ensure_callback_mutable(id)?;
        let (mut values, temporary) = self.with_native::<SequencePayload, _>(id, |sequence| {
            let temporary = self
                .resources
                .reserve_temporary_heap(sequence.values.len())?;
            Ok((
                sequence.values.copy_range(0, sequence.values.len())?,
                temporary,
            ))
        })?;
        let mut active = self.mutations.borrow_mut();
        active
            .try_reserve(1)
            .map_err(|_| self.resource_limit("sequence edit registry"))?;
        active.insert(id, 1);
        drop(active);
        let guard = MutationLease {
            active: self.mutations.clone(),
            id,
        };
        let result = edit(SequenceEdit {
            values: &mut values,
        })?;
        drop(guard);
        self.resources.poll_execution()?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self
            .objects
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("conflicting sequence edit borrow"))?;
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
        sequence.values = values;
        objects[id.slot].revision = revision;
        drop(temporary);
        Ok(result)
    }
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid sequence edit receiver")
}
