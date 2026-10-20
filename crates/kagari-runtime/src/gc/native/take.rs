//! Successful extraction detaches storage and preserves inert moved identities.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId},
    native::{binding::NativeResult, payload::take::NativeTake},
};

impl GcHeap {
    pub(crate) fn take_native<T: NativeTake>(&self, id: HeapObjectId) -> NativeResult<T> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        if self.native_operations.is_active(id) {
            return Err(RuntimeError::module_validation(
                "native extraction during an active operation",
            ));
        }
        let object = {
            let mut objects = self.objects_mut()?;
            let Some(object) = self.native_ref(&objects, id) else {
                return Err(RuntimeError::module_validation(
                    "invalid native extraction receiver",
                ));
            };
            if !object.storage.is_takeable() || !object.storage.accepts_payload::<T>() {
                return Err(RuntimeError::module_validation(
                    "storage does not permit payload extraction",
                ));
            }
            object.payload::<T>()?;
            let charge = object.charge;
            let mut moved = self.moved.try_borrow_mut().map_err(|_| {
                RuntimeError::module_validation("moved identity registry is borrowed")
            })?;
            moved
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("moved identity registry"))?;
            let mut free = self
                .free
                .try_borrow_mut()
                .map_err(|_| RuntimeError::module_validation("heap free slots are borrowed"))?;
            free.try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("heap free slots"))?;
            let Some(object) = objects.detach_native(id) else {
                unreachable!("validated native extraction receiver")
            };
            moved.insert(id);
            let slot = &mut objects.slots[id.index()];
            if let Some(generation) = slot.generation.checked_add(1) {
                slot.generation = generation;
                free.push(id.index());
            }
            // Keep the existing header unit for the inert identity until its last
            // traced alias disappears. Payload storage is no longer heap-owned.
            self.resources.release_heap_charge(charge);
            object
        };
        // Metadata and descriptor destruction occur with no storage-table borrow.
        Ok(object.into_payload::<T>())
    }
}
