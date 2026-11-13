//! A queued factory owns data independently of conversion and live frame suffixes.
#[cfg(feature = "execution-diagnostics")]
use super::diagnostics;
use super::reserve_bank;
#[cfg(test)]
mod tests;
use crate::{
    execution::roots::{SessionId, Slots, ValueBank, ValueLocation},
    value::carriers::{ScopedValue, StoredValue},
};
use kagari_gc::error::GcError;

pub(super) struct QueueRecord {
    sequence: u64,
    count: usize,
}

/// Linear ownership of one cold queue's argument bank. Views borrow this token;
/// frame admission copies its inputs before the queue owner retires the bank.
#[derive(Debug)]
pub(crate) struct QueuedArguments {
    execution: SessionId,
    sequence: u64,
    count: usize,
}

impl QueuedArguments {
    pub(crate) fn len(&self) -> usize {
        self.count
    }

    pub(crate) fn check_bounds(&self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        let storage = slots.storage(self.execution)?;
        if storage
            .queue
            .as_ref()
            .is_none_or(|record| record.sequence != self.sequence || record.count != self.count)
            || storage.banks[ValueBank::Queued.index()].len() != self.count
        {
            return Err(GcError::Storage("queued argument owner expired"));
        }
        Ok(())
    }

    pub(crate) fn location(&self, index: usize) -> Option<ValueLocation> {
        (index < self.count).then_some(ValueLocation {
            execution: self.execution,
            bank: ValueBank::Queued,
            slot: index,
        })
    }

    pub(crate) fn get<'gc>(
        &self,
        slots: &mut Slots<'_, 'gc>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        self.check_bounds(slots)?;
        slots.get(self.location(index).ok_or(GcError::Index)?)
    }
}

impl<'gc> Slots<'_, 'gc> {
    /// The conversion owner admits every original before reserving this separate
    /// bank. Partial publication remains in one closed operation and rolls back
    /// through retire_queue before returning any error.
    pub(crate) fn begin_queue(
        &mut self,
        execution: SessionId,
        count: usize,
    ) -> Result<QueuedArguments, GcError> {
        let storage = self.storage(execution)?;
        let values = &mut storage.banks[ValueBank::Queued.index()];
        if storage.queue.is_some()
            || !values.is_empty()
            || storage.frame_depth != 0
            || !storage.native_calls.is_empty()
        {
            return Err(GcError::Storage("factory queue already owned or executing"));
        }
        let sequence = storage
            .queue_sequence
            .checked_add(1)
            .ok_or(GcError::Limit("queued argument identity"))?;
        reserve_bank(values, count)?;
        values.resize_with(count, StoredValue::unit);
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::initialized::<StoredValue>(count);
        storage.queue_sequence = sequence;
        storage.queue = Some(QueueRecord { sequence, count });
        Ok(QueuedArguments {
            execution,
            sequence,
            count,
        })
    }

    pub(crate) fn set_queued(
        &mut self,
        owner: &QueuedArguments,
        index: usize,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        owner.check_bounds(self)?;
        self.set(owner.location(index).ok_or(GcError::Index)?, value)
    }

    /// The next frame/source owns every value now. This bank is independent of
    /// frame/local marks, so clearing it cannot retire live callee scratch.
    pub(crate) fn retire_queue(&mut self, owner: QueuedArguments) -> Result<(), GcError> {
        owner.check_bounds(self)?;
        let storage = self.storage(owner.execution)?;
        storage.banks[ValueBank::Queued.index()].clear();
        storage.queue = None;
        Ok(())
    }
}
