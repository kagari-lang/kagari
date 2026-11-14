//! Range instructions copy scalar endpoints and publish their result in the frame.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    error_trace::ErrorTrace,
    frame::{FrameStack, instruction::Inputs, types::compatibility::TypeView},
    range::RangeValue,
    value::{
        aggregates::enumerations::EnumObject, carriers::ScopedValue, kind::ObjectKind,
        view::ValueView,
    },
};
use kagari_bytecode::instruction::Slot;
use kagari_gc::error::GcError;
use kagari_types::ty::Ty;
use std::{ops::Bound, slice};

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

    pub(crate) fn range_bound(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        value: Slot,
        range: &Ty,
        bound: &Ty,
        upper: bool,
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, slice::from_ref(&value), destination)?;
        let value = inputs.access(runtime, |cx| {
            let value = cx.field(0)?;
            if value.kind() != Some(ObjectKind::Range) {
                return Err(RuntimeError::from(TrapReason::InvalidRangeValue).into());
            }
            let object = cx
                .slots
                .scope()
                .downcast::<RangeValue>(value.object().ok_or(GcError::Representation)?)?;
            Ok(cx.slots.scope().read(object, |range| *range))
        })?;
        let frame = self.current(runtime)?;
        let owner = frame.loaded().clone();
        let range = frame
            .type_arguments(runtime, slice::from_ref(range))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("range type scope"))?;
        let bound = frame
            .type_arguments(runtime, slice::from_ref(bound))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("range bound type scope"))?;
        drop(frame);
        // RangeValue contains only immutable scalar bits. No heap/source loan is
        // held during nominal member lookup or detached error-origin capture.
        let selected = value.bound(bound.definitions(), range.ty(), bound.ty(), upper)?;
        let (member, field) = match selected {
            Bound::Included(value) => (
                "Included",
                Some(
                    ValueView::protected(value, None)
                        .map_err(|error| runtime.managed_memory_error(error))?,
                ),
            ),
            Bound::Excluded(value) => (
                "Excluded",
                Some(
                    ValueView::protected(value, None)
                        .map_err(|error| runtime.managed_memory_error(error))?,
                ),
            ),
            Bound::Unbounded => ("Unbounded", None),
        };
        let layout = runtime.declared_enum_variant(&owner, &bound, member)?;
        inputs.access(runtime, |cx| {
            if layout.variant().payload.len() != usize::from(field.is_some()) {
                return Err(RuntimeError::from(TrapReason::InvalidRangeValue).into());
            }
            if let Some(field) = field {
                let field = field.get(cx.slots)?;
                let (ty, environment) = layout.payload_type(0).ok_or(GcError::Representation)?;
                cx.check(
                    field,
                    TypeView::new(ty, layout.module(), environment),
                    TrapReason::InvalidRangeValue,
                )?;
            }
            Ok(())
        })?;
        let origin = layout
            .variant()
            .reports_failure
            .then(|| ErrorTrace::capture(&runtime.resources));
        inputs.write(self, runtime, |cx| {
            let mut fields = [ScopedValue::unit()];
            let count = if let Some(field) = field {
                fields[0] = field.get(cx.slots)?;
                1
            } else {
                0
            };
            let object = EnumObject::alloc(cx.slots.scope(), layout, &fields[..count], origin)?;
            Ok(ScopedValue::managed(object))
        })
    }
}
