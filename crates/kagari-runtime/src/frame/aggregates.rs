//! Cold aggregate instructions publish directly into their admitted frame slots.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    error_trace::ErrorTrace,
    frame::{
        FrameStack,
        instruction::{Inputs, InstructionScope},
        types::compatibility::TypeView,
    },
    module::EnumVariantRef,
    value::{
        aggregates::{enumerations::EnumObject, structures::StructObject, tuples::TupleObject},
        arrays::{ArrayObject, storage::ArrayValues},
        carriers::ScopedValue,
        kind::ObjectKind,
    },
    value_check::scoped::CheckError,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::Slot;
use kagari_gc::error::GcError;
use kagari_types::ty::Ty;
use std::slice;

const CHUNK: usize = 1024;

impl InstructionScope<'_, '_, '_, '_> {
    fn check_enum(&mut self, layout: &EnumVariantRef) -> Result<(), CheckError> {
        if self.len() != layout.variant().payload.len() {
            return Err(RuntimeError::from(TrapReason::InvalidHeapTargetIndexOrPayload).into());
        }
        for index in 0..self.len() {
            let value = self.field(index)?;
            let (ty, environment) = layout.payload_type(index).ok_or(GcError::Representation)?;
            self.check(
                value,
                TypeView::new(ty, layout.module(), environment),
                TrapReason::InvalidHeapTargetIndexOrPayload,
            )?;
        }
        Ok(())
    }
}

impl FrameStack {
    pub(crate) fn make_tuple(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        fields: &[Slot],
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, fields, destination)?;
        inputs.write(self, runtime, |cx| {
            let fields = cx.fields()?;
            let object = TupleObject::alloc(cx.slots.scope(), &fields)?;
            Ok(ScopedValue::managed(object))
        })
    }

    pub(crate) fn make_struct(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        fields: &[Slot],
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, fields, destination)?;
        let layout = self.current(runtime)?.struct_layout(runtime)?;
        let description = runtime.structure_layouts.prepare(layout)?;
        if fields.len() != description.fields.len() {
            return Err(RuntimeError::from(
                TrapReason::InvalidHeapTargetIndexOrPayload,
            ));
        }
        inputs.write(self, runtime, |cx| {
            let fields = cx.fields()?;
            for (index, &value) in fields.iter().enumerate() {
                let (ty, environment) = description
                    .semantic
                    .field_type(index)
                    .ok_or(GcError::Representation)?;
                cx.check(
                    value,
                    TypeView::new(ty, description.semantic.module(), environment),
                    TrapReason::InvalidHeapTargetIndexOrPayload,
                )?;
            }
            let object = StructObject::alloc(
                cx.slots.scope(),
                description,
                &fields,
                cx.resources.candidate_origin(),
            )?;
            Ok(ScopedValue::managed(object))
        })
    }

    pub(crate) fn make_enum(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        fields: &[Slot],
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, fields, destination)?;
        let layout = self.current(runtime)?.enum_variant(runtime)?;
        // Exact source coordinates remain owned by this frame. The only work
        // between semantic admission and allocation is readonly trace capture;
        // it cannot change fields, the frame or the selected immutable layout.
        inputs.access(runtime, |cx| cx.check_enum(&layout))?;
        let origin = layout
            .variant()
            .reports_failure
            .then(|| ErrorTrace::capture(&runtime.resources));
        inputs.write(self, runtime, |cx| {
            let fields = cx.fields()?;
            let object = EnumObject::alloc(cx.slots.scope(), layout, &fields, origin)?;
            Ok(ScopedValue::managed(object))
        })
    }

    pub(crate) fn make_array(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        element: &Ty,
        fields: &[Slot],
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, fields, destination)?;
        let contract = self.current(runtime)?.element_contract(runtime, element)?;
        inputs.write(self, runtime, |cx| {
            let mut values = ArrayValues::empty(&contract.ty);
            values.reserve(cx.len())?;
            for index in 0..cx.len() {
                let value = cx.field(index)?;
                cx.check(
                    value,
                    contract.view(),
                    TrapReason::InvalidArrayTargetOrPayload,
                )?;
                values.push(cx.slots.scope(), value)?;
            }
            cx.resources.poll_closed()?;
            let object = ArrayObject::alloc(
                cx.slots.scope(),
                contract,
                values,
                cx.resources.candidate_origin(),
            )?;
            Ok(ScopedValue::managed(object))
        })
    }

    pub(crate) fn repeat_array(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        element: &Ty,
        value: Slot,
        count: Slot,
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, &[value, count], destination)?;
        let count = inputs.access(runtime, |cx| match cx.field(1)?.scalar_payload() {
            Some((ValueType::U64, count)) => Ok(count),
            _ => Err(
                RuntimeError::from(TrapReason::Provider("invalid repeat array count".into()))
                    .into(),
            ),
        })?;
        let contract = self.current(runtime)?.element_contract(runtime, element)?;
        inputs.write(self, runtime, |cx| {
            let value = cx.field(0)?;
            cx.check(
                value,
                contract.view(),
                TrapReason::InvalidArrayTargetOrPayload,
            )?;
            let count = usize::try_from(count)
                .map_err(|_| GcError::Limit("array length exceeds platform capacity"))?;
            let mut values = ArrayValues::empty(&contract.ty);
            values.reserve(count)?;
            for start in (0..count).step_by(CHUNK) {
                cx.resources.poll_closed()?;
                values.repeat(cx.slots.scope(), value, (count - start).min(CHUNK))?;
            }
            cx.resources.poll_closed()?;
            let object = ArrayObject::alloc(
                cx.slots.scope(),
                contract,
                values,
                cx.resources.candidate_origin(),
            )?;
            Ok(ScopedValue::managed(object))
        })
    }

    pub(crate) fn test_enum_variant(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        value: Slot,
    ) -> Result<(), RuntimeError> {
        self.enum_field(runtime, destination, value, None)
    }

    pub(crate) fn read_enum_payload(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        value: Slot,
        index: u32,
    ) -> Result<(), RuntimeError> {
        self.enum_field(runtime, destination, value, Some(index))
    }

    fn enum_field(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        value: Slot,
        field: Option<u32>,
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, slice::from_ref(&value), destination)?;
        let layout = self.current(runtime)?.enum_variant(runtime)?;
        inputs.write(self, runtime, |cx| {
            let value = cx.field(0)?;
            if value.kind() != Some(ObjectKind::Enum) {
                return Err(RuntimeError::from(TrapReason::Provider(
                    "enum pattern expects enum value".into(),
                ))
                .into());
            }
            let object = cx
                .slots
                .scope()
                .downcast::<EnumObject>(value.object().ok_or(GcError::Representation)?)?;
            let matches = cx
                .slots
                .scope()
                .read(object, |object| object.layout().matches_layout(&layout));
            let Some(field) = field else {
                return Ok(ScopedValue::from(matches));
            };
            if !matches {
                return Err(RuntimeError::from(TrapReason::Provider(
                    "enum pattern variant mismatch".into(),
                ))
                .into());
            }
            let field = usize::try_from(field)
                .ok()
                .filter(|&field| field < cx.slots.scope().read(object, EnumObject::len))
                .ok_or_else(|| {
                    RuntimeError::from(TrapReason::Provider("invalid enum payload index".into()))
                })?;
            Ok(EnumObject::get(cx.slots.scope(), object, field)?)
        })
    }
}
