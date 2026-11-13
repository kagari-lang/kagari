//! Struct conversion publishes fields into source storage before child callbacks.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, SessionId, Slots, ValueBank, ValueLocation},
    frame::types::compatibility::TypeView,
    module::StructLayoutRef,
    native::{
        binding::NativeResult,
        conversion::context::{ConversionContext, inputs::ValueInput},
    },
    value::{
        aggregates::structures::StructObject,
        carriers::{RootedValue, ScopedValue},
        view::ValueView,
    },
    value_check::{
        self,
        scoped::{self, CheckError},
    },
};
use kagari_gc::{error::GcError, object::Gc};

/// The snapshot's only values live in the enclosing conversion's local suffix.
/// This record owns no copied pack, payload address or independent root lease.
#[derive(Debug)]
pub(crate) struct StructSnapshot {
    execution: SessionId,
    start: usize,
    len: usize,
}

impl StructSnapshot {
    pub(crate) fn element<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        index: usize,
    ) -> NativeResult<ValueView<'call>> {
        if index >= self.len {
            return Err(RuntimeError::module_validation("struct snapshot field"));
        }
        let location = ValueLocation {
            execution: self.execution,
            bank: ValueBank::Local,
            slot: self.start + index,
        };
        cx.with_values(|slots| {
            let value = slots.get(location)?;
            ValueView::protected(value, value.object().map(|_| location))
        })
    }
}

impl<'runtime> ConversionContext<'runtime> {
    pub(crate) fn allocate_struct<'root, 'call>(
        &mut self,
        layout: StructLayoutRef,
        values: impl ExactSizeIterator<Item = ValueInput<'root, 'call>>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.poll()?;
        if !layout.module().belongs_to(self.runtime.host.owner()) {
            return Err(RuntimeError::module_validation("foreign struct layout"));
        }
        let description = self.runtime.structure_layouts.prepare(layout)?;
        if values.len() != description.fields.len() {
            return Err(RuntimeError::module_validation("struct initializer length"));
        }
        let execution = self.execution_id()?;
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let mut input = Vec::new();
                input.try_reserve_exact(values.len())?;
                for (slot, value) in values.enumerate() {
                    let value = value.get(slots)?;
                    let (ty, environment) = description
                        .semantic
                        .field_type(slot)
                        .ok_or(GcError::Representation)?;
                    let matched = match scoped::matches(
                        slots.scope(),
                        value,
                        TypeView::new(ty, description.semantic.module(), environment),
                        host,
                        host_borrows,
                        resources,
                    ) {
                        Ok(matched) => matched,
                        Err(CheckError::Storage(error)) => return Err(error),
                        Err(CheckError::Execution(error)) => return Ok(Err(error)),
                    };
                    if !matched {
                        return Err(GcError::Representation);
                    }
                    input.push(value);
                }
                let object = StructObject::alloc(
                    slots.scope(),
                    description,
                    &input,
                    resources.candidate_origin(),
                )?;
                let value = ScopedValue::managed(object);
                let location = slots.push_local(execution, value)?;
                ValueView::protected(value, Some(location)).map(Ok)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub(crate) fn struct_layout(&mut self, value: &ValueView<'_>) -> NativeResult<StructLayoutRef> {
        value_check::candidate_protected(self.runtime, &[value.reborrow()])?;
        self.with_values(|slots| {
            let object = structure(slots, *value)?;
            Ok(slots.scope().read(object, |object| object.layout().clone()))
        })
    }

    pub(crate) fn check_struct_root(
        &mut self,
        root: &RootedValue,
        expected: &StructLayoutRef,
    ) -> NativeResult<()> {
        value_check::candidate_root(self.runtime, root)?;
        self.with_values(|slots| {
            let value = root
                .get(slots.scope())?
                .object()
                .ok_or(GcError::Representation)?;
            let object = slots.scope().downcast::<StructObject>(value)?;
            if !slots
                .scope()
                .read(object, |object| object.layout().matches(expected))
            {
                return Err(GcError::Representation);
            }
            Ok(())
        })
    }

    pub(crate) fn struct_field(
        &mut self,
        value: &ValueView<'_>,
        expected: &StructLayoutRef,
        slot: usize,
    ) -> NativeResult<ValueView<'runtime>> {
        value_check::candidate_protected(self.runtime, &[value.reborrow()])?;
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let object = structure(slots, *value)?;
            if !slots
                .scope()
                .read(object, |object| object.layout().matches(expected))
            {
                return Err(GcError::Representation);
            }
            let value = StructObject::get(slots.scope(), object, slot)?;
            let location = if value.object().is_some() {
                Some(slots.push_local(execution, value)?)
            } else {
                None
            };
            ValueView::protected(value, location)
        })
    }

    /// The encoder/verified operation has checked the nominal field value. This
    /// writer retains physical representation, mutability and layout admission.
    pub(crate) fn set_struct_field(
        &mut self,
        receiver: &ValueView<'_>,
        expected: &StructLayoutRef,
        slot: usize,
        value: &ValueView<'_>,
    ) -> NativeResult<()> {
        value_check::candidate_protected(self.runtime, &[receiver.reborrow(), value.reborrow()])?;
        self.with_values(|slots| {
            let value = value.get(slots)?;
            let object = structure(slots, *receiver)?;
            StructObject::set(slots.scope(), object, expected, slot, value)
        })
    }

    /// Capture every field before the first decoder can mutate an input alias.
    /// The caller owns a scratch Frame that retires the entire snapshot suffix.
    pub(crate) fn snapshot_struct(
        &mut self,
        value: &ValueView<'_>,
        expected: &StructLayoutRef,
    ) -> NativeResult<StructSnapshot> {
        value_check::candidate_protected(self.runtime, &[value.reborrow()])?;
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let object = structure(slots, *value)?;
            let len = slots.scope().read(object, |object| {
                if !object.layout().matches(expected) {
                    return Err(GcError::Representation);
                }
                Ok(object.len())
            })?;
            let start = slots.mark_locals(execution)?.top();
            for slot in 0..len {
                let value = StructObject::get(slots.scope(), object, slot)?;
                slots.push_local(execution, value)?;
            }
            Ok(StructSnapshot {
                execution,
                start,
                len,
            })
        })
    }
}

fn structure<'gc>(
    slots: &mut Slots<'_, 'gc>,
    value: ValueView<'_>,
) -> Result<Gc<'gc, StructObject>, GcError> {
    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
    slots.scope().downcast::<StructObject>(value)
}
