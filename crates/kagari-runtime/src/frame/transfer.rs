//! Return packets retain snapshots in the callee's existing protection owner.
use crate::{
    execution::roots::{
        SessionId, Slots,
        frames::{FrameBanks, FrameMark},
        returns::ReturnSnapshot,
    },
    frame::values::operands::write_operand,
    module::execution::layout::Location,
    value::{
        carriers::{RootedValue, ScopedValue},
        scalar::{self, domain},
        view::ValueView,
    },
};
use kagari_abi::representation::ValueType;
use kagari_gc::error::GcError;

/// A linear return transition. A source-backed packet starts in its callee;
/// finishing a local stack transfers it into the surrounding activation's source.
/// Popping/discarding an owner without that transfer expires the packet. Host
/// ownership requires explicit retention; the packet is no independent root lease.
#[derive(Debug)]
pub struct ReturnValue(Contents);

#[derive(Debug)]
enum Contents {
    Protected(ReturnSnapshot),
    Retained(RootedValue),
    Scalar {
        representation: ValueType,
        bits: u64,
    },
}

impl ReturnValue {
    /// Acquire escaping Host ownership, reusing an existing retained packet's
    /// lease after authenticating it in this heap. Ordinary source packets gain
    /// a lease only at this explicit ownership boundary.
    pub(crate) fn into_retained(
        self,
        slots: &mut Slots<'_, '_>,
        execution: SessionId,
    ) -> Result<RootedValue, GcError> {
        match self.0 {
            Contents::Retained(value) => {
                value.get(slots.scope())?;
                Ok(value)
            }
            contents => Self(contents).read(slots, execution)?.retain(slots.scope()),
        }
    }

    /// Move a local execution-stack result to its still-live surrounding source
    /// owner. The caller retires this adjusted mark in the same closed scope.
    pub(crate) fn reparent(
        self,
        slots: &mut Slots<'_, '_>,
        callee: &mut FrameMark,
    ) -> Result<Self, GcError> {
        slots.check_frame_exit(callee)?;
        if let Contents::Protected(_) = self.0 {
            let value = self.read(slots, callee.banks().execution)?;
            return slots
                .publish_parent_return(callee, value)
                .map(Contents::Protected)
                .map(Self);
        }
        Ok(self)
    }

    /// Supply an explicitly retained Host result to a manually driven return.
    /// Ordinary interpreter returns use the frame's source, not this Host route.
    pub fn retained(value: RootedValue) -> Self {
        Self(Contents::Retained(value))
    }

    pub(crate) fn scalar(representation: ValueType, bits: u64) -> Self {
        Self(Contents::Scalar {
            representation,
            bits,
        })
    }

    pub(super) fn is_scalar(&self) -> bool {
        matches!(self.0, Contents::Scalar { .. })
    }

    /// Snapshot before result adaptation can execute or overwrite another slot.
    /// The frame's mark owns this local cell through commit/trap/unwind cleanup.
    pub(crate) fn protect<'gc>(
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        value: ScopedValue<'gc>,
    ) -> Result<Self, GcError> {
        if let Some((representation, bits)) = value.scalar_payload() {
            return Ok(Self::scalar(representation, bits));
        }
        slots
            .snapshot_return(execution, value)
            .map(Contents::Protected)
            .map(Self)
    }

    #[cfg(test)]
    pub(crate) fn payload(&self) -> Option<(ValueType, u64)> {
        match self.0 {
            Contents::Scalar {
                representation,
                bits,
            } => Some((representation, bits)),
            Contents::Protected(_) | Contents::Retained(_) => None,
        }
    }

    pub(crate) fn read<'gc>(
        &self,
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
    ) -> Result<ScopedValue<'gc>, GcError> {
        match &self.0 {
            Contents::Protected(snapshot) => {
                let location = slots.return_location(*snapshot, execution)?;
                slots.get(location)
            }
            Contents::Retained(value) => value.get(slots.scope()),
            Contents::Scalar {
                representation,
                bits,
            } => {
                if !domain::admits_payload(*representation, None, *bits) {
                    return Err(GcError::Representation);
                }
                scalar::decode(*representation, *bits).ok_or(GcError::Representation)
            }
        }
    }

    /// Publish into the immediately preceding caller while the callee still
    /// owns its snapshot and executable dependencies. Every fallible admission
    /// precedes the caller write; the caller retires the checked mark afterwards
    /// in the same collection-free scope, without invoking an adapter.
    pub(crate) fn publish_to_caller(
        &self,
        slots: &mut Slots<'_, '_>,
        callee: &FrameMark,
        caller: FrameBanks,
        destination: Option<Location>,
    ) -> Result<(), GcError> {
        slots.check_frame_exit(callee)?;
        let banks = callee.banks();
        if caller.execution != banks.execution
            || caller.managed.checked_add(caller.managed_count) != Some(banks.managed)
            || caller.scalars.checked_add(caller.scalar_count) != Some(banks.scalars)
        {
            return Err(GcError::Storage(
                "return destination is not the preceding caller",
            ));
        }
        slots.check_frame_banks(caller)?;
        if let Some(destination) = destination {
            let value = self.read(slots, banks.execution)?;
            write_operand(slots, caller, destination, value)?;
        }
        Ok(())
    }

    /// Reopen a nonescaping argument for an adapter. A Host-retained packet is
    /// imported into this callee's protection before lending a managed view.
    pub(crate) fn view<'value>(
        &self,
        slots: &mut Slots<'_, '_>,
        execution: SessionId,
    ) -> Result<ValueView<'value>, GcError> {
        if let Contents::Protected(snapshot) = self.0 {
            let location = slots.return_location(snapshot, execution)?;
            let value = slots.get(location)?;
            return ValueView::protected(value, Some(location));
        }
        let value = self.read(slots, execution)?;
        let location = if value.object().is_some() {
            Some(slots.push_local(execution, value)?)
        } else {
            None
        };
        ValueView::protected(value, location)
    }
}

#[cfg(test)]
mod tests;
