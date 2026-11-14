//! Bounded data-free parking inspects source tags, never managed pointers.
use crate::{
    execution::roots::{DependencyId, ExecutionRoots, SessionId, frames::FrameBanks},
    module::managed::revision::RevisionState,
    value::carriers::StoredValue,
};
use kagari_gc::error::GcError;

impl ExecutionRoots {
    /// The caller supplies its actual session's admitted revision prefix. With
    /// that as the sole dependency, inline banks contain no captured data to
    /// inspect. Additional dependencies may own receiver data and require the
    /// ordinary physical graph walk, even when every frame operand is scalar.
    /// This result is valid only during this read; no admission is cached.
    pub(crate) fn inline_parking(
        &self,
        execution: SessionId,
        body: DependencyId<RevisionState>,
        frames: impl Iterator<Item = FrameBanks>,
    ) -> Result<bool, GcError> {
        let storage = self.inspect_storage(execution)?;
        if body.execution != execution
            || body.slot != 0
            || storage.dependencies.len() != 1
            || !storage.native_calls.is_empty()
        {
            return Ok(false);
        }
        // Large ranges keep the existing cooperatively checked graph path.
        let mut remaining = 1024usize;
        for bank in &storage.banks {
            let Some(next) = remaining.checked_sub(bank.len()) else {
                return Ok(false);
            };
            remaining = next;
            if !bank.iter().all(StoredValue::is_inline_scalar) {
                return Ok(false);
            }
        }
        for banks in frames {
            let Some(next) = remaining.checked_sub(1) else {
                return Ok(false);
            };
            remaining = next;
            if banks.execution != execution || !storage.contains_frame_banks(banks) {
                return Err(GcError::Index);
            }
        }
        Ok(true)
    }
}
