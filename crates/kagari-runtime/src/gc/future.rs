//! Claiming removes cold captures without lending a heap borrow to submission.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    native::{
        binding::NativeResult,
        future::{ColdFuture, FuturePayload},
    },
};

impl GcHeap {
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
        self.release_heap_units(cold.values.len());
        Ok(cold)
    }
}
