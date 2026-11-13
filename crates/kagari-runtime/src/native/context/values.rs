//! Raw Native views reopen source-protected providers only for closed operations.
use crate::{
    execution::roots::{ExecutionRoots, Slots},
    native::{
        binding::NativeResult, context::NativeCall, conversion::context::ConversionContext,
        storage::NativePayload, views::ValueHandle,
    },
    value::{
        carriers::{RootedValue, ScopedValue},
        kind::ObjectKind,
        native::NativeObject,
        properties::ValueProperties,
        view::ValueView,
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject},
};

impl<'call> NativeCall<'call> {
    /// Borrow an argument's prepared type and source without an owning root.
    pub fn value_handle(&mut self, index: usize) -> NativeResult<ValueHandle<'call>> {
        ValueHandle::from_argument(self, index)
    }

    /// Publish a concrete provider's selected script field into this Native
    /// source before releasing its payload. The selector cannot execute Runtime
    /// services or return a detached stored edge. No Host root is registered.
    pub fn value_payload<S: GcObject>(
        &mut self,
        receiver: &ValueView<'_>,
        select: for<'gc> fn(&Scope<'gc>, Gc<'gc, S>) -> Result<ScopedValue<'gc>, GcError>,
    ) -> NativeResult<ValueView<'call>> {
        let execution = self.pending.execution();
        self.with_payload_source(receiver, |slots, payload| {
            let value = select(slots.scope(), payload)?;
            if !ValueProperties::of(slots.scope(), value)?.storable {
                return Err(GcError::Representation);
            }
            let location = value
                .object()
                .map(|_| slots.push_local(execution, value))
                .transpose()?;
            ValueView::protected(value, location)
        })
    }

    /// Reopen one source-authenticated concrete payload. The closed operation
    /// owns any child publication; its result cannot carry this GC brand.
    pub(in crate::native) fn with_payload_source<S: GcObject, R>(
        &mut self,
        receiver: &ValueView<'_>,
        access: impl for<'gc> FnOnce(&mut Slots<'_, 'gc>, Gc<'gc, S>) -> Result<R, GcError>,
    ) -> NativeResult<R> {
        self.poll()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = receiver.get(slots)?;
                    if value.kind() != Some(ObjectKind::Native) {
                        return Err(GcError::Representation);
                    }
                    let object = slots
                        .scope()
                        .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
                    if !slots
                        .scope()
                        .read(object, |object| object.storage().matches_payload::<S>())
                    {
                        return Err(GcError::Representation);
                    }
                    let payload = NativeObject::payload(slots.scope(), object);
                    let payload = slots.scope().downcast::<S>(payload)?;
                    access(slots, payload)
                })
            })
            .map_err(|error| self.gc_error(error))
    }

    /// A short conversion may collect or allocate preparation cells. Promote
    /// only its completed result before cleanup, under this call's Native mark.
    pub(super) fn convert_value(
        &mut self,
        operation: impl for<'conversion> FnOnce(
            &mut ConversionContext<'conversion>,
        ) -> NativeResult<ValueView<'conversion>>,
    ) -> NativeResult<ValueView<'call>> {
        let execution = self.pending.execution();
        let mut conversion = ConversionContext::in_native_call(self)?;
        let value = operation(&mut conversion)?;
        let value = conversion.into_native_value(execution, value)?;
        // The consumed conversion moved its retirement boundary past this value.
        // NativeCall's enclosing mark, rather than the short Rust borrow, now
        // owns its protection through return/error/unwind and pending commit.
        Ok(value.reborrow())
    }

    pub(in crate::native) fn validate_value<'value>(
        &mut self,
        value: ValueView<'value>,
    ) -> NativeResult<ValueView<'value>> {
        self.runtime.resources.ensure_synchronous_thread()?;
        self.runtime.resources.ensure_execution_allowed()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| value.get(slots).map(|_| value))
            })
            .map_err(|error| self.gc_error(error))
    }

    pub(in crate::native) fn read_payload<S: NativePayload, R>(
        &mut self,
        value: ValueView<'_>,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.with_native_value(value, |cx, object| {
            let payload = NativeObject::payload(cx, object);
            cx.read(object, |object| {
                object.storage().read_payload(cx, payload, access)
            })
        })?
    }

    pub(in crate::native) fn retain_value(
        &mut self,
        value: ValueView<'_>,
    ) -> NativeResult<RootedValue> {
        self.runtime.resources.ensure_synchronous_thread()?;
        self.runtime.resources.ensure_execution_allowed()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?;
                    if !ValueProperties::of(slots.scope(), value)?.storable {
                        return Err(GcError::Representation);
                    }
                    value.retain(slots.scope())
                })
            })
            .map_err(|error| self.gc_error(error))
    }

    pub(in crate::native) fn enter_native_operation(
        &mut self,
        value: ValueView<'_>,
    ) -> NativeResult<bool> {
        self.with_native_value(value, NativeObject::enter_operation)
    }

    /// Cleanup deliberately bypasses execution/cancellation admission. No payload
    /// or source borrow spans the operation callback, including script reentry.
    pub(in crate::native) fn leave_native_operation(
        &mut self,
        value: ValueView<'_>,
    ) -> Result<(), GcError> {
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?;
                    let object = slots
                        .scope()
                        .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
                    NativeObject::leave_operation(slots.scope(), object)
                })
            })
    }

    fn with_native_value<R>(
        &mut self,
        value: ValueView<'_>,
        operation: impl for<'gc> FnOnce(&mut Scope<'gc>, Gc<'gc, NativeObject>) -> Result<R, GcError>,
    ) -> NativeResult<R> {
        self.runtime.resources.ensure_synchronous_thread()?;
        self.runtime.resources.ensure_execution_allowed()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?;
                    if value.kind() != Some(ObjectKind::Native) {
                        return Err(GcError::Representation);
                    }
                    let object = slots
                        .scope()
                        .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
                    operation(slots.scope(), object)
                })
            })
            .map_err(|error| self.gc_error(error))
    }
}
