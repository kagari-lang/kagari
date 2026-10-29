//! Successful extraction detaches storage and preserves inert moved identities.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, graph::GraphIdentity},
    native::{binding::NativeResult, payload::take::NativeTake},
};

impl GcHeap {
    pub(crate) fn take_native<T: NativeTake>(&self, id: HeapObjectId) -> NativeResult<T> {
        self.ensure_no_storage_borrow()?;
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        if self.native_operations.is_active(id) {
            return Err(RuntimeError::module_validation(
                "native extraction during an active operation",
            ));
        }
        let mut retired = {
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
            moved.prepare(id)?;

            let mut graph = self
                .graph
                .try_lock()
                .map_err(|_| RuntimeError::module_validation("graph admission is borrowed"))?;
            let location = self
                .object_location(&objects, id)
                .expect("checked take identity");
            let source_mark = objects.mark(location).expect("occupied take mark");
            if !graph.accessible(source_mark) {
                return Err(RuntimeError::module_validation(
                    "condemned native extraction receiver",
                ));
            }
            graph.prepare_identity()?;
            let mark = moved.insert_prepared(id);
            graph.admit_moved_prepared(GraphIdentity::Moved(id), source_mark, mark);
            let Some(retired) = objects.detach(id) else {
                unreachable!("validated native extraction receiver")
            };
            graph.retire_prepared();

            // Keep the existing header unit for the inert identity until its last
            // traced alias disappears. Payload storage is no longer heap-owned.
            self.resources.release_heap_charge(charge);
            retired
        };
        let payload = retired.take_native::<T>();
        // Metadata and descriptor destruction occur with no storage-table borrow.
        self.dispose_payload(retired);
        Ok(payload)
    }
}
