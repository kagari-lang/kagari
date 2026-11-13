//! One cleared backing set can survive execution retirement without managed roots.
#[cfg(feature = "execution-diagnostics")]
use super::diagnostics;
use crate::{
    execution::{
        roots::{ExecutionStorage, pending, reserve_bank},
        space::ExecutionSpace,
    },
    value::carriers::StoredValue,
};
use kagari_gc::error::GcError;
use std::sync::Arc;

impl ExecutionStorage {
    /// Both fallible reservations precede initialization/publication. On failure
    /// the cache still contains only empty buffers, ready for another admission.
    pub(super) fn prepare(&mut self, managed: usize, limit: usize) -> Result<(), GcError> {
        let reusable_space = self.space.take();
        if self.backing_bytes() > limit {
            *self = Self::default();
        }
        let space = match reusable_space {
            Some(mut space) => {
                if let Some(account) = Arc::get_mut(&mut space) {
                    account.reset(limit);
                    space
                } else {
                    Arc::new(ExecutionSpace::new(limit))
                }
            }
            None => Arc::new(ExecutionSpace::new(limit)),
        };
        space.claim(self.backing_bytes())?;
        self.space = Some(space.clone());
        let [values, _, _, pending] = &mut self.banks;
        debug_assert!(values.is_empty() && pending.is_empty());
        reserve_bank(values, managed, &space)?;
        reserve_bank(pending, pending::SLOT_COUNT, &space)?;
        values.resize_with(managed, StoredValue::unit);
        pending.resize_with(pending::SLOT_COUNT, StoredValue::unit);
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::initialized::<StoredValue>(managed + pending::SLOT_COUNT);
        Ok(())
    }

    /// Retire all logical state and inert edges, retaining only Vec capacity.
    /// No callback, payload access or managed destructor runs while clearing.
    pub(super) fn clear(&mut self) {
        if let Some(space) = &self.space {
            space.release(self.backing_bytes());
        }
        for bank in &mut self.banks {
            bank.clear();
        }
        self.scalars.clear();
        self.initialized.clear();
        self.native_calls.clear();
        self.dependencies.clear();
        self.frame_depth = 0;
        self.returns.clear();
        self.return_sequence = 0;
        self.queue_sequence = 0;
        self.queue = None;
    }
}
