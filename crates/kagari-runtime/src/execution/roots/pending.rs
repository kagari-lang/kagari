//! One owned wait keeps its awaitable and completion in fixed source cells.
use crate::{
    execution::roots::{SessionId, Slots, ValueBank, ValueLocation},
    value::carriers::ScopedValue,
};
use kagari_gc::error::GcError;

pub(super) const SLOT_COUNT: usize = 2;

/// Coordinates only. Session control admits at most one wait, and the complete
/// execution identity prevents these cells from being reused by another owner.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingSlots {
    execution: SessionId,
}

impl PendingSlots {
    pub(crate) fn new(execution: SessionId) -> Self {
        Self { execution }
    }

    pub(crate) fn output(self) -> ValueLocation {
        ValueLocation {
            execution: self.execution,
            bank: ValueBank::Pending,
            slot: 0,
        }
    }

    pub(crate) fn awaitable(self) -> ValueLocation {
        ValueLocation {
            slot: 1,
            ..self.output()
        }
    }

    pub(crate) fn release_awaitable(self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        slots.set(self.awaitable(), ScopedValue::unit())
    }

    /// The caller publishes a result to its next owner before clearing these
    /// cells. Retirement also drops both with the execution, including on traps.
    pub(crate) fn clear(self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        slots.get(self.output())?;
        slots.get(self.awaitable())?;
        slots.set(self.output(), ScopedValue::unit())?;
        self.release_awaitable(slots)
    }
}
