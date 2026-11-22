//! Range construction copies scalar endpoints and publishes the range in the frame.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{FrameStack, instruction::Inputs},
    range::RangeValue,
    value::carriers::ScopedValue,
};
use kagari_bytecode::instruction::Slot;
use kagari_types::ty::Ty;
use std::slice;

impl FrameStack {
    pub(crate) fn make_range(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        ty: &Ty,
        start: Option<Slot>,
        end: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        // Padding is never admitted or read: the selected slice contains exactly
        // the authored endpoints, with start preceding end whenever both exist.
        let (fields, len, start, end) = match (start, end) {
            (Some(start), Some(end)) => ([start, end], 2, Some(0), Some(1)),
            (Some(start), None) => ([start, start], 1, Some(0), None),
            (None, Some(end)) => ([end, end], 1, None, Some(0)),
            (None, None) => ([Slot::new(0); 2], 0, None, None),
        };
        let inputs = Inputs::prepare(self, runtime, &fields[..len], destination)?;
        let ty = self
            .current(runtime)?
            .type_arguments(runtime, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("range type scope"))?;
        inputs.write(self, runtime, |cx| {
            let start = start.map(|index| cx.field(index)).transpose()?;
            let end = end.map(|index| cx.field(index)).transpose()?;
            let range = RangeValue::new(ty.ty(), start, end).map_err(RuntimeError::from)?;
            Ok(ScopedValue::managed(range.alloc(cx.slots.scope())?))
        })
    }
}
