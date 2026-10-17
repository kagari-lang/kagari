//! Borrowed banks preserve the ordinary slot representation rules after window admission.
use crate::{
    frame::values::{ExecutionValues, FrameSlots},
    module::execution::layout::Location,
    value::{Value, scalar},
};
use kagari_abi::representation::ValueType;

/// A checked source window whose identity cannot change during the storage borrow.
pub(crate) struct OperandView<'a> {
    managed: &'a [Value],
    payloads: &'a [u64],
    initialized: &'a [bool],
}

pub(crate) struct OperandWindow<'a> {
    pub(crate) managed: &'a mut [Value],
    pub(crate) payloads: &'a mut [u64],
    pub(crate) initialized: &'a mut [bool],
}

impl ExecutionValues {
    pub(crate) fn read_operands(&self, slots: FrameSlots) -> Option<OperandView<'_>> {
        let ranges = &self.window(slots)?.ranges;
        Some(OperandView {
            managed: self.values.get(ranges.managed.clone())?,
            payloads: self.payloads.get(ranges.scalars.clone())?,
            initialized: self.initialized.get(ranges.scalars.clone())?,
        })
    }

    pub(crate) fn borrow_operands(&mut self, slots: FrameSlots) -> Option<OperandWindow<'_>> {
        // Validate owner/generation once, before any references into the banks escape.
        self.window(slots)?;
        let window = self.windows.get(slots.index)?;
        window.registers.as_ref()?;
        Some(OperandWindow {
            managed: self.values.get_mut(window.ranges.managed.clone())?,
            payloads: self.payloads.get_mut(window.ranges.scalars.clone())?,
            initialized: self.initialized.get_mut(window.ranges.scalars.clone())?,
        })
    }
}

impl OperandView<'_> {
    pub(crate) fn with_value<R>(
        &self,
        location: Location,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        with_operand(
            self.managed,
            self.payloads,
            self.initialized,
            location,
            read,
        )
    }

    fn scalar_location(&self, location: Location) -> Option<(ValueType, u64)> {
        let slot = location.operand.scalar()?.index();
        if !self.initialized.get(slot).copied()? {
            return None;
        }
        Some((location.representation, *self.payloads.get(slot)?))
    }

    pub(crate) fn check_managed(
        &self,
        location: Location,
        check: impl FnOnce(&Value) -> bool,
    ) -> Option<bool> {
        if location.operand.scalar().is_some() {
            self.scalar_location(location)?;
            return Some(true);
        }
        self.with_value(location, check)
    }
}

impl OperandWindow<'_> {
    // Consume the projection in its caller, including the shared decoder below.
    #[inline(always)]
    pub(crate) fn with_value<R>(
        &self,
        location: Location,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        with_operand(
            self.managed,
            self.payloads,
            self.initialized,
            location,
            read,
        )
    }

    pub(crate) fn read_location(&self, location: Location) -> Option<Value> {
        read_operand(self.managed, self.payloads, self.initialized, location)
    }

    pub(crate) fn write_location(&mut self, location: Location, value: Value) -> Option<()> {
        write_operand(
            self.managed,
            self.payloads,
            self.initialized,
            location,
            value,
        )
    }
}

pub(super) fn read_operand(
    managed: &[Value],
    payloads: &[u64],
    initialized: &[bool],
    location: Location,
) -> Option<Value> {
    with_operand(managed, payloads, initialized, location, |value| *value)
}

// Projection is a compile-time abstraction: do not transport its generic result
// across an extra call before the caller can eliminate unused Value variants.
#[inline(always)]
fn with_operand<R>(
    managed: &[Value],
    payloads: &[u64],
    initialized: &[bool],
    location: Location,
    read: impl FnOnce(&Value) -> R,
) -> Option<R> {
    let slot = location.operand;
    if slot.managed() {
        return managed.get(slot.index()).map(read);
    }
    if !initialized.get(slot.index()).copied()? {
        // Inspection preserves unavailable Unit; scalar kernels reject missing payloads.
        return Some(read(&Value::Unit));
    }
    let value = scalar::decode(location.representation, *payloads.get(slot.index())?)?;
    Some(read(&value))
}

pub(super) fn write_operand(
    managed: &mut [Value],
    payloads: &mut [u64],
    initialized: &mut [bool],
    location: Location,
    value: Value,
) -> Option<()> {
    if !location.admits(&value) {
        return None;
    }
    let slot = location.operand;
    if slot.managed() {
        *managed.get_mut(slot.index())? = value;
    } else {
        *payloads.get_mut(slot.index())? = scalar::encode(&value)?;
        *initialized.get_mut(slot.index())? = true;
    }
    Some(())
}
