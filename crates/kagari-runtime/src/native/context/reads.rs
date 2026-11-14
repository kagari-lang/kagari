//! Closed Native reads borrow actual sources and publish selected children.
use crate::{
    execution::roots::ExecutionRoots,
    module::EnumVariantRef,
    native::{binding::NativeResult, context::NativeCall},
    value::{
        aggregates::enumerations::EnumObject, arrays::ArrayObject, carriers::ScopedValue,
        strings::StringObject, view::ValueView,
    },
    value_check,
};
use kagari_gc::error::GcError;

impl<'call> NativeCall<'call> {
    /// Publish immutable UTF-8 into this call's local source without a Host root.
    pub fn allocate_string(&mut self, text: String) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        let execution = self.pending.execution();
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let object = StringObject::alloc(slots.scope(), text)?;
                    slots.protect_local(execution, ScopedValue::managed(object))
                })
            })
            .map_err(|error| self.gc_error(error))
    }

    /// Borrow fixed UTF-8 inputs together without copying or registering roots.
    /// The callback has no mutable Runtime services and cannot retain its loans.
    pub fn read_strings<R, const N: usize>(
        &mut self,
        values: &[ValueView<'_>; N],
        read: impl FnOnce([&str; N]) -> R,
    ) -> NativeResult<R> {
        self.poll()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let mut scoped = [ScopedValue::unit(); N];
                    for (destination, value) in scoped.iter_mut().zip(values) {
                        *destination = value.get(slots)?;
                    }
                    StringObject::read_many(slots.scope(), &scoped, read)
                })
            })
            .map_err(|error| self.gc_error(error))
    }

    /// Read the admitted fixed-array length without preparing a conversion.
    pub fn array_len(&mut self, value: &ValueView<'_>) -> NativeResult<usize> {
        self.poll()?;
        value_check::candidate_protected(self.runtime, &[*value])?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
                    let array = slots.scope().downcast::<ArrayObject>(value)?;
                    Ok(slots.scope().read(array, ArrayObject::len))
                })
            })
            .map_err(|error| self.gc_error(error))
    }

    /// Publish a present element into the enclosing Native source before return.
    /// An unrepresentable host index is absent after receiver admission.
    pub fn array_element(
        &mut self,
        value: &ValueView<'_>,
        index: impl TryInto<usize>,
    ) -> NativeResult<Option<ValueView<'call>>> {
        self.poll()?;
        value_check::candidate_protected(self.runtime, &[*value])?;
        let index = index.try_into().ok();
        let execution = self.pending.execution();
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
                    let array = slots.scope().downcast::<ArrayObject>(value)?;
                    let length = slots.scope().read(array, ArrayObject::len);
                    let Some(index) = index.filter(|index| *index < length) else {
                        return Ok(None);
                    };
                    let value = ArrayObject::get(slots.scope(), array, index)?;
                    slots.protect_local(execution, value).map(Some)
                })
            })
            .map_err(|error| self.gc_error(error))
    }

    /// Read immutable nominal member facts and payload count from an actual enum.
    /// This grants no independent executable source or payload ownership.
    pub fn enum_shape(&mut self, value: &ValueView<'_>) -> NativeResult<(EnumVariantRef, usize)> {
        self.poll()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
                    let object = slots.scope().downcast::<EnumObject>(value)?;
                    Ok(slots
                        .scope()
                        .read(object, |object| (object.layout().clone(), object.len())))
                })
            })
            .map_err(|error| self.gc_error(error))
    }

    /// Protect one physical payload field through this Native call's lifetime.
    /// Callers validate the nominal member before interpreting its payload.
    pub fn enum_field(
        &mut self,
        value: &ValueView<'_>,
        field: usize,
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        let execution = self.pending.execution();
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
                    let object = slots.scope().downcast::<EnumObject>(value)?;
                    let value = EnumObject::get(slots.scope(), object, field)?;
                    slots.protect_local(execution, value)
                })
            })
            .map_err(|error| self.gc_error(error))
    }
}
