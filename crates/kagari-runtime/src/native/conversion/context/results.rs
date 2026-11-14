//! A caller conversion owns result destinations through synchronous child cleanup.
use crate::{
    error::RuntimeError,
    execution::roots::{SessionId, ValueLocation},
    native::{
        binding::NativeResult,
        conversion::context::{ConversionContext, preserve_value},
    },
    value::{
        carriers::ScopedValue,
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
    value_check,
};

impl<'call> ConversionContext<'call> {
    /// Transfer one raw allocation/field result to the enclosing Native suffix.
    /// Consuming this context prevents a later append from moving that boundary.
    /// Host contexts must retain/publish through their own explicit result API.
    pub(in crate::native) fn into_native_value(
        mut self,
        execution: SessionId,
        value: ValueView<'call>,
    ) -> NativeResult<ValueView<'call>> {
        if self.owns_execution || self.activation.is_some() || self.execution != Some(execution) {
            return Err(RuntimeError::module_validation(
                "conversion has no Native parent",
            ));
        }
        value_check::candidate_protected(self.runtime, &[value])?;
        let mark = self.locals.as_mut().expect("Native conversion suffix");
        let result = preserve_value(self.runtime, mark, value);
        result.map_err(|error| self.gc_error(error))
    }

    /// Reserve before child scratch starts. This context's suffix owns retirement
    /// on success, error and unwind; no independent root or logical frame is added.
    pub(in crate::native) fn result_place(
        &mut self,
    ) -> NativeResult<(ValueLocation, ResultPlace<'call>)> {
        let execution = self.execution_id()?;
        let location =
            self.with_values(|slots| slots.push_local(execution, ScopedValue::unit()))?;
        Ok((location, ResultPlace::new(location)))
    }

    pub(in crate::native) fn published_result(
        &mut self,
        published: PublishedResult<'_>,
        expected: ValueLocation,
    ) -> NativeResult<ValueView<'call>> {
        if expected.execution != self.execution_id()? {
            return Err(RuntimeError::module_validation(
                "foreign conversion result destination",
            ));
        }
        let location = published.admit(expected)?;
        self.with_values(|slots| {
            let value = slots.get(location)?;
            ValueView::protected(value, Some(location))
        })
    }
}
