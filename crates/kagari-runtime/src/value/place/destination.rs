//! Publication targets borrow coordinate descriptors, never source or payload loans.
use crate::{
    execution::roots::{Slots, ValueLocation, frames::FrameBanks},
    frame::values::operands::write_operand,
    module::execution::layout::Location,
    value::{carriers::ScopedValue, view::ValueView},
};
use kagari_gc::error::GcError;
use std::ptr;

/// A frame owner lends stack-local copies of its coordinates and prepared result
/// facts across the callback. Dropping a result still admits the source/frame;
/// it does not add a local cell, root lease or a write to any operand bank.
#[derive(Clone, Copy)]
pub(super) enum Destination<'call> {
    Source(ValueLocation),
    Frame(&'call FrameBanks, Option<&'call Location>),
}

impl Destination<'_> {
    pub(super) fn publish(
        self,
        slots: &mut Slots<'_, '_>,
        value: &ValueView<'_>,
    ) -> Result<(), GcError> {
        self.check(slots)?;
        let value = value.get(slots)?;
        self.write(slots, value)
    }

    pub(super) fn publish_scoped<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        self.check(slots)?;
        self.write(slots, value)
    }

    fn check(self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        match self {
            Self::Source(location) => slots.get(location).map(|_| ()),
            Self::Frame(frame, _) => slots.check_frame_banks(*frame),
        }
    }

    fn write<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        match self {
            Self::Source(location) => slots.set(location, value),
            Self::Frame(frame, location) => {
                if let Some(location) = location {
                    write_operand(slots, *frame, *location, value)?;
                }
                Ok(())
            }
        }
    }

    pub(super) fn admits_source(self, expected: ValueLocation) -> bool {
        matches!(self, Self::Source(location) if location == expected)
    }

    /// The proof must name the exact descriptors lent by this call owner. This
    /// compares ordinary Rust references, not managed addresses or retention IDs.
    pub(super) fn admits_frame(self, expected: &FrameBanks, location: Option<&Location>) -> bool {
        match self {
            Self::Frame(frame, actual) if ptr::eq(frame, expected) => match (actual, location) {
                (Some(actual), Some(expected)) => ptr::eq(actual, expected),
                (None, None) => true,
                _ => false,
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
