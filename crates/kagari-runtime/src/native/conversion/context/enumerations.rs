//! Declared enum conversion prepares nominal facts before physical admission.
use crate::{
    Runtime,
    error::RuntimeError,
    error_trace::ErrorTrace,
    execution::roots::{ExecutionRoots, Slots, ValueBank},
    frame::types::arguments::TypeArgument,
    frame::types::compatibility::TypeView,
    module::EnumVariantRef,
    native::{
        binding::NativeResult,
        conversion::{
            arguments::ArgumentValues,
            context::{ConversionContext, inputs::ValueInput},
        },
    },
    value::{aggregates::enumerations::EnumObject, carriers::ScopedValue, view::ValueView},
    value_check::scoped::{self, CheckError},
};
use kagari_gc::{error::GcError, object::Gc};

impl<'runtime> ConversionContext<'runtime> {
    /// Copy a value's immutable payload with the original carrier's detached
    /// diagnostic origin. This metadata does not affect equality or hashing.
    pub(crate) fn forward_enum_origin(
        &mut self,
        original: &ValueView<'_>,
        value: &ValueView<'_>,
    ) -> NativeResult<ValueView<'runtime>> {
        let execution = self.execution_id()?;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let original = enumeration(slots, *original)?;
                let value = enumeration(slots, *value)?;
                let origin = slots
                    .scope()
                    .read(original, |original| original.origin().cloned());
                let (layout, count) = slots
                    .scope()
                    .read(value, |value| (value.layout().clone(), value.len()));
                let mut fields = Vec::new();
                fields.try_reserve_exact(count)?;
                for index in 0..count {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    fields.push(EnumObject::get(slots.scope(), value, index)?);
                }
                let object = EnumObject::alloc(slots.scope(), layout, &fields, origin)?;
                let value = ScopedValue::managed(object);
                let location = slots.push_local(execution, value)?;
                ValueView::protected(value, Some(location)).map(Ok)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub(crate) fn allocate_enum_inputs<'root, 'call>(
        &mut self,
        layout: EnumVariantRef,
        values: impl ExactSizeIterator<Item = ValueInput<'root, 'call>>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.poll()?;
        if !layout.module().belongs_to(self.runtime.host.owner()) {
            return Err(RuntimeError::module_validation("foreign enum layout"));
        }
        if values.len() != layout.variant().payload.len() {
            return Err(RuntimeError::module_validation("enum payload length"));
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
                let mut fields = Vec::new();
                fields.try_reserve_exact(values.len())?;
                for (index, value) in values.enumerate() {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let value = value.get(slots)?;
                    let (ty, environment) =
                        layout.payload_type(index).ok_or(GcError::Representation)?;
                    let matched = match scoped::matches(
                        slots.scope(),
                        value,
                        TypeView::new(ty, layout.module(), environment),
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
                    fields.push(value);
                }
                let origin = layout
                    .variant()
                    .reports_failure
                    .then(|| ErrorTrace::capture(resources));
                let value = EnumObject::alloc(slots.scope(), layout, &fields, origin)?;
                let value = ScopedValue::managed(value);
                let location = slots.push_local(execution, value)?;
                ValueView::protected(value, Some(location)).map(Ok)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub(crate) fn allocate_enum_pack(
        &mut self,
        layout: EnumVariantRef,
        values: ArgumentValues<'runtime>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.validate_arguments(values)?;
        self.allocate_enum_inputs(
            layout,
            (0..values.len()).map(|index| ValueInput::Pack(values, index)),
        )
    }

    /// Snapshot the entire immutable payload before any child converter executes.
    /// The caller's scratch owner retires these coordinates on every exit.
    pub(crate) fn enum_arguments(
        &mut self,
        value: &ValueView<'_>,
    ) -> NativeResult<ArgumentValues<'runtime>> {
        let count = self.with_values(|slots| {
            let object = enumeration(slots, *value)?;
            Ok(slots.scope().read(object, EnumObject::len))
        })?;
        self.check_elements(count)?;
        let execution = self.execution_id()?;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let object = enumeration(slots, *value)?;
                let start = slots.len(execution, ValueBank::Local)?;
                slots.reserve_locals(execution, count)?;
                for index in 0..count {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let value = EnumObject::get(slots.scope(), object, index)?;
                    slots.push_local(execution, value)?;
                }
                Ok(Ok(ArgumentValues::new(execution, start, count)))
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    /// A fixed Rust adapter uses stack scratch. Payload conversion and all
    /// nominal checks finish before origin capture or allocation has effects.
    pub(crate) fn allocate_enum_member<const N: usize>(
        &mut self,
        applied: &TypeArgument,
        member: &str,
        values: &[ValueView<'_>; N],
    ) -> NativeResult<ValueView<'runtime>> {
        let layout = self
            .runtime
            .declared_enum_variant(self.owner, applied, member)?;
        if layout.variant().payload.len() != N {
            return Err(RuntimeError::module_validation("enum payload length"));
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
                let mut fields = [ScopedValue::unit(); N];
                for (slot, (field, value)) in fields.iter_mut().zip(values).enumerate() {
                    *field = value.get(slots)?;
                    let (ty, environment) =
                        layout.payload_type(slot).ok_or(GcError::Representation)?;
                    let matched = match scoped::matches(
                        slots.scope(),
                        *field,
                        TypeView::new(ty, layout.module(), environment),
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
                }
                // Trace capture reads detached execution metadata only, after all
                // checks and with no payload loan or callback/reentry in flight.
                let origin = layout
                    .variant()
                    .reports_failure
                    .then(|| ErrorTrace::capture(resources));
                let value = EnumObject::alloc(slots.scope(), layout, &fields, origin)?;
                let value = ScopedValue::managed(value);
                let location = slots.push_local(execution, value)?;
                ValueView::protected(value, Some(location)).map(Ok)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub(crate) fn enum_shape(
        &mut self,
        value: &ValueView<'_>,
    ) -> NativeResult<(EnumVariantRef, usize)> {
        self.with_values(|slots| {
            let object = enumeration(slots, *value)?;
            Ok(slots
                .scope()
                .read(object, |object| (object.layout().clone(), object.len())))
        })
    }

    /// Protect the selected child before its decoder may collect or reenter.
    /// No whole-payload clone, Host root or heap loan crosses that boundary.
    pub(crate) fn enum_element(
        &mut self,
        value: &ValueView<'_>,
        index: usize,
    ) -> NativeResult<ValueView<'runtime>> {
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let object = enumeration(slots, *value)?;
            let value = EnumObject::get(slots.scope(), object, index)?;
            slots.protect_local(execution, value)
        })
    }
}

fn enumeration<'gc>(
    slots: &mut Slots<'_, 'gc>,
    value: ValueView<'_>,
) -> Result<Gc<'gc, EnumObject>, GcError> {
    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
    slots.scope().downcast::<EnumObject>(value)
}
