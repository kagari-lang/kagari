//! Borrowed banks preserve the ordinary slot representation rules after window admission.
use crate::{
    frame::values::{ExecutionValues, FrameSlots, scalar},
    module::execution::layout::{FrameLayout, Location},
    value::Value,
};

pub(crate) struct OperandWindow<'a> {
    layout: &'a FrameLayout,
    pub(crate) managed: &'a mut [Value],
    pub(crate) payloads: &'a mut [u64],
    pub(crate) initialized: &'a mut [bool],
}

impl ExecutionValues {
    pub(crate) fn borrow_operands(&mut self, slots: FrameSlots) -> Option<OperandWindow<'_>> {
        // Validate owner/generation once, before any references into the banks escape.
        self.window(slots)?;
        let window = self.windows.get(slots.index)?.as_ref()?;
        Some(OperandWindow {
            layout: window.registers.as_deref()?,
            managed: self.values.get_mut(window.ranges.managed.clone())?,
            payloads: self.payloads.get_mut(window.ranges.scalars.clone())?,
            initialized: self.initialized.get_mut(window.ranges.scalars.clone())?,
        })
    }
}

impl OperandWindow<'_> {
    pub(crate) fn read(&self, logical: usize) -> Option<Value> {
        read_operand(
            self.managed,
            self.payloads,
            self.initialized,
            self.layout.location(logical)?,
        )
    }

    pub(crate) fn write(&mut self, logical: usize, value: Value) -> Option<()> {
        write_operand(
            self.managed,
            self.payloads,
            self.initialized,
            self.layout.location(logical)?,
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
    let slot = location.operand;
    if slot.managed() {
        return managed.get(slot.index()).copied();
    }
    if !initialized.get(slot.index()).copied()? {
        // Inspection preserves unavailable Unit; scalar kernels reject missing payloads.
        return Some(Value::Unit);
    }
    scalar::decode(location.representation, *payloads.get(slot.index())?)
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
