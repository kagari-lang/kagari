//! Closed operand access publishes managed writes through the paired source.
use crate::{
    execution::roots::{Slots, ValueBank, ValueLocation, frames::FrameBanks},
    frame::transfer::ReturnValue,
    module::execution::{layout::Location, operands::OperandSlot},
    value::{
        carriers::ScopedValue,
        scalar::{self, domain},
    },
};
use kagari_gc::{access::Scope, error::GcError};

/// This loan cannot cross GC or arbitrary executing services. Managed fields
/// remain encapsulated; scalar-only regions may lend the disjoint payload banks.
pub(crate) struct OperandWindow<'access, 'source, 'gc> {
    pub(super) slots: &'access mut Slots<'source, 'gc>,
    pub(super) banks: FrameBanks,
}

impl<'gc> OperandWindow<'_, '_, 'gc> {
    pub(crate) fn read_managed(
        &mut self,
        operand: OperandSlot,
    ) -> Result<ScopedValue<'gc>, GcError> {
        if !operand.managed() || operand.index() >= self.banks.managed_count {
            return Err(GcError::Index);
        }
        self.slots.get(ValueLocation {
            execution: self.banks.execution,
            bank: ValueBank::Managed,
            slot: self.banks.managed + operand.index(),
        })
    }

    pub(crate) fn scope(&mut self) -> &mut Scope<'gc> {
        self.slots.scope()
    }

    pub(crate) fn protect_return(
        &mut self,
        value: ScopedValue<'gc>,
    ) -> Result<ReturnValue, GcError> {
        ReturnValue::protect(self.slots, self.banks.execution, value)
    }

    pub(crate) fn read_location(
        &mut self,
        location: Location,
    ) -> Result<ScopedValue<'gc>, GcError> {
        read_operand(self.slots, self.banks, location)
    }

    pub(crate) fn write_location(
        &mut self,
        location: Location,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        write_operand(self.slots, self.banks, location, value)
    }

    /// No managed edge or Runtime capability accompanies these scalar slices.
    pub(crate) fn scalar_banks(&mut self) -> Result<(&mut [u64], &mut [bool]), GcError> {
        self.slots.check_frame_banks(self.banks)?;
        let storage = self.slots.storage(self.banks.execution)?;
        let range = self.banks.scalars..self.banks.scalars + self.banks.scalar_count;
        Ok((
            &mut storage.scalars[range.clone()],
            &mut storage.initialized[range],
        ))
    }
}

#[inline(always)]
pub(super) fn read_operand<'gc>(
    slots: &mut Slots<'_, 'gc>,
    banks: FrameBanks,
    location: Location,
) -> Result<ScopedValue<'gc>, GcError> {
    slots.check_frame_banks(banks)?;
    if location.operand.is_unit() {
        return location
            .admits(ScopedValue::unit())
            .then_some(ScopedValue::unit())
            .ok_or(GcError::Representation);
    }
    let index = location.operand.index();
    if location.operand.managed() {
        if index >= banks.managed_count {
            return Err(GcError::Index);
        }
        let value = slots.get(ValueLocation {
            execution: banks.execution,
            bank: ValueBank::Managed,
            slot: banks.managed + index,
        })?;
        // Public cold inspection preserves unavailable Unit. Arguments/typed
        // operations separately require the prepared actual representation.
        if !value.is_unit() && !location.admits(value) {
            return Err(GcError::Representation);
        }
        return Ok(value);
    }
    if index >= banks.scalar_count {
        return Err(GcError::Index);
    }
    let storage = slots.storage(banks.execution)?;
    let index = banks.scalars + index;
    if !storage.initialized[index] {
        return Ok(ScopedValue::unit());
    }
    let bits = storage.scalars[index];
    if !domain::admits_payload(location.representation, location.semantic, bits) {
        return Err(GcError::Representation);
    }
    scalar::decode(location.representation, bits).ok_or(GcError::Representation)
}

pub(crate) fn write_operand<'gc>(
    slots: &mut Slots<'_, 'gc>,
    banks: FrameBanks,
    location: Location,
    value: ScopedValue<'gc>,
) -> Result<(), GcError> {
    slots.check_frame_banks(banks)?;
    if !location.admits(value) {
        return Err(GcError::Representation);
    }
    if location.operand.is_unit() {
        return Ok(());
    }
    let index = location.operand.index();
    if location.operand.managed() {
        if index >= banks.managed_count {
            return Err(GcError::Index);
        }
        slots.set(
            ValueLocation {
                execution: banks.execution,
                bank: ValueBank::Managed,
                slot: banks.managed + index,
            },
            value,
        )
    } else {
        if index >= banks.scalar_count {
            return Err(GcError::Index);
        }
        let bits = scalar::encode(&value).ok_or(GcError::Representation)?;
        let storage = slots.storage(banks.execution)?;
        storage.scalars[banks.scalars + index] = bits;
        storage.initialized[banks.scalars + index] = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
