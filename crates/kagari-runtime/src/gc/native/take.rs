//! Extraction transfers T once and leaves its original allocation traceable.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, reference::GcRef},
    native::{binding::NativeResult, payload::take::NativeTake},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

impl GcHeap {
    pub(crate) fn take_native<T: NativeTake>(&self, id: GcRef) -> NativeResult<T> {
        self.ensure_no_storage_borrow()?;
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        if self.native_operations.is_active(id) {
            return Err(RuntimeError::module_validation(
                "native extraction during an active operation",
            ));
        }
        let (payload, native, descriptor) = {
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
            let taken = objects.take_native::<T>(id)?;
            // Payload ownership moves to the host. The header unit and actual
            // physical backing remain charged until the last alias is collected.
            self.resources.release_heap_charge(charge);
            taken
        };
        // Neither metadata destruction nor user-owned factory capture Drop may
        // inherit the storage borrow. T has already moved and cannot drop twice.
        if catch_unwind(AssertUnwindSafe(|| drop((native, descriptor)))).is_err() {
            self.resources
                .quarantine("taken native metadata destruction panicked");
        }
        Ok(payload)
    }
}
