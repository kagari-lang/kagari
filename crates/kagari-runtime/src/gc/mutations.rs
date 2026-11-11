//! Guards prevent callbacks from modifying prepared mutation targets through aliases.

use crate::{
    error::{RuntimeError, trap::TrapReason},
    gc::{GcHeap, GcObjectKind, HeapObjectId, NativeMutation},
    value::Value,
};

impl GcHeap {
    pub(crate) fn begin_native_mutation(
        &self,
        value: &Value,
    ) -> Result<NativeMutation, RuntimeError> {
        self.ensure_execution_allowed()?;
        let id = match value {
            Value::GcHandle(id) if self.object_kind(*id) == Some(GcObjectKind::Native) => *id,

            _ => {
                return Err(RuntimeError::from(TrapReason::InvalidMutationTarget));
            }
        };
        self.ensure_callback_mutable(id)?;
        let root = self
            .root_value(*value)
            .ok_or_else(|| RuntimeError::from(TrapReason::InvalidMutationRoot))?;
        let lease = self
            .mutations
            .acquire(id)
            .map_err(|_| self.resource_limit("mutation registry"))?;
        Ok(NativeMutation {
            _lease: lease,
            _root: root,
        })
    }

    pub(crate) fn ensure_callback_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.mutations.is_active(id) {
            Err(RuntimeError::from(
                TrapReason::ContainerMutationDuringAGuardedCallback,
            ))
        } else {
            Ok(())
        }
    }
}
