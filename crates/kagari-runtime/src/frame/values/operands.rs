//! Borrowed banks preserve the ordinary slot representation rules after window admission.
use crate::{
    frame::values::{ExecutionValues, FrameSlots},
    gc::GcHeap,
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

/// Execution permission is admitted once for a closed interpreter borrow. No
/// callback, collection, native loan, or error recovery may intervene before this
/// window is dropped. Every managed write still validates and publishes its value.
pub(in crate::frame) struct OperandWindow<'a> {
    heap: &'a GcHeap,
    managed: &'a mut [Value],
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

    pub(in crate::frame) fn borrow_operands<'a>(
        &'a mut self,
        heap: &'a GcHeap,
        slots: FrameSlots,
    ) -> Option<OperandWindow<'a>> {
        // Validate owner/generation once, before any references into the banks escape.
        self.window(slots)?;
        let window = self.windows.get(slots.index)?;
        window.slot_layout.as_ref()?;
        heap.ensure_execution_allowed().ok()?;
        Some(OperandWindow {
            heap,
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
    pub(crate) fn managed_value(&self, index: usize) -> Option<Value> {
        self.managed.get(index).copied()
    }

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
            self.heap,
            self.managed,
            self.payloads,
            self.initialized,
            location,
            value,
            || Some(()),
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

// Only internal permission actions are supplied here: a standalone write checks
// execution after representation/bounds preflight; a closed OperandWindow already
// admitted it. Keep that ordering because permission rejection can quarantine.
pub(super) fn write_operand(
    heap: &GcHeap,
    managed: &mut [Value],
    payloads: &mut [u64],
    initialized: &mut [bool],
    location: Location,
    value: Value,
    admit_managed_write: impl FnOnce() -> Option<()>,
) -> Option<()> {
    if !location.admits(&value) {
        return None;
    }
    let slot = location.operand;
    if slot.managed() {
        let destination = managed.get_mut(slot.index())?;
        admit_managed_write()?;
        // Every managed bank publication, including direct frame transfers, must
        // protect a new edge even when the root cursor has passed this slot.
        heap.publish_value(value).ok()?;
        *destination = value;
    } else {
        let destination = payloads.get_mut(slot.index())?;
        let initialized = initialized.get_mut(slot.index())?;
        *destination = scalar::encode(&value)?;
        *initialized = true;
    }
    Some(())
}
