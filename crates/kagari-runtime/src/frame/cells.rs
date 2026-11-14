//! Capture instructions share one real cell identity and its publication barrier.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    frame::{FrameStack, instruction::Inputs},
    value::{
        carriers::ScopedValue, cells::CellObject, kind::ObjectKind, properties::ValueProperties,
    },
    value_check::scoped::CheckError,
};
use kagari_bytecode::instruction::Slot;
use kagari_gc::error::GcError;
use std::slice;

impl FrameStack {
    pub(crate) fn make_cell(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        value: Slot,
    ) -> Result<(), RuntimeError> {
        let representation = self.current(runtime)?.slot_type(runtime, value)?;
        let inputs = Inputs::prepare(self, runtime, slice::from_ref(&value), destination)?;
        inputs.write(self, runtime, |cx| {
            let value = cx.field(0)?;
            if !value.has_representation(representation)
                || !ValueProperties::of(cx.slots.scope(), value)?.storable
            {
                return Err(RuntimeError::from(TrapReason::InvalidCellValue).into());
            }
            let cell = CellObject::alloc(
                cx.slots.scope(),
                representation,
                value,
                cx.resources.candidate_origin(),
            )?;
            Ok(ScopedValue::managed(cell))
        })
    }

    pub(crate) fn read_cell(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        cell: Slot,
    ) -> Result<(), RuntimeError> {
        let representation = self.current(runtime)?.slot_type(runtime, destination)?;
        let inputs = Inputs::prepare(self, runtime, slice::from_ref(&cell), destination)?;
        inputs.write(self, runtime, |cx| {
            let value = cx.field(0)?;
            if value.kind() != Some(ObjectKind::Cell) {
                return Err(RuntimeError::from(TrapReason::ExpectedCaptureCell).into());
            }
            let cell = cx
                .slots
                .scope()
                .downcast::<CellObject>(value.object().ok_or(GcError::Representation)?)?;
            CellObject::get(cx.slots.scope(), cell, representation).map_err(capture_error)
        })
    }

    pub(crate) fn write_cell(
        &self,
        runtime: &mut Runtime,
        cell: Slot,
        value: Slot,
    ) -> Result<(), RuntimeError> {
        let representation = self.current(runtime)?.slot_type(runtime, value)?;
        let inputs = Inputs::prepare_effect(self, runtime, &[cell, value])?;
        inputs.effect(self, runtime, |cx| {
            let cell = cx.field(0)?;
            let value = cx.field(1)?;
            if cell.kind() != Some(ObjectKind::Cell) {
                return Err(RuntimeError::from(TrapReason::ExpectedCaptureCell).into());
            }
            // A tagged cell's incoming value precedes target/contract validation.
            // InvalidCellValue and InvalidCaptureCell remain distinct traps.
            if !value.has_representation(representation)
                || !ValueProperties::of(cx.slots.scope(), value)?.storable
            {
                return Err(RuntimeError::from(TrapReason::InvalidCellValue).into());
            }
            let cell = cx
                .slots
                .scope()
                .downcast::<CellObject>(cell.object().ok_or(GcError::Representation)?)?;
            cx.resources.poll_closed()?;
            CellObject::set(cx.slots.scope(), cell, representation, value).map_err(capture_error)
        })
    }
}

fn capture_error(error: GcError) -> CheckError {
    match error {
        GcError::Representation => RuntimeError::from(TrapReason::InvalidCaptureCell).into(),
        error => error.into(),
    }
}
