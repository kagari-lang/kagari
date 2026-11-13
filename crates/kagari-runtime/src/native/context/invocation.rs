//! One admitted Native context owns execution, completion precedence and publication.
use crate::{
    error::RuntimeError,
    native::{
        binding::{BindingEntry, NativeResult},
        context::NativeCall,
        conversion::context::ConversionContext,
    },
    value::place::{PublishedResult, ResultPlace},
    value_check,
};

impl NativeCall<'_> {
    /// Consume this admitted invocation even when the body or encoder fails.
    /// A retry cannot run arbitrary Rust a second time after completed effects.
    pub(crate) fn invoke<'result>(
        &mut self,
        destination: ResultPlace<'result>,
    ) -> NativeResult<PublishedResult<'result>> {
        self.pending.begin_invocation()?;
        if self.arguments.len() != self.function.signature.params.len() {
            return Err(RuntimeError::module_validation(
                "native invocation argument count",
            ));
        }
        self.native_stack = Some(self.runtime.resources.enter_native_stack()?);
        let result = self
            .narrow_arguments()
            .and_then(|()| self.invoke_narrowed(destination));
        result.map_err(|error| self.runtime.resources().admit_execution_error(error))
    }

    fn invoke_narrowed<'result>(
        &mut self,
        destination: ResultPlace<'result>,
    ) -> NativeResult<PublishedResult<'result>> {
        let function = self.function;
        let signature = function.type_signature()?;
        let result = match &function.binding.entry {
            BindingEntry::Callback(entry) => entry(self).map_err(|error| *error),
            BindingEntry::Primitive(primitive) => {
                let arguments = self.arguments();
                primitive
                    .execute(self.runtime, arguments)
                    .map_err(|error| *error)
                    .and_then(|value| self.set_result(value))
            }
        }
        .map_err(|error| self.runtime.resources().admit_execution_error(error));
        // Preserve the existing post-call precedence, including callback Err.
        self.poll()?;
        result?;
        let value = self.result()?;
        if !function.binding.converted_result || function.result_adapter.is_some() {
            value_check::check_protected(self.runtime, self.owner, &signature.result, &value)?;
        }
        match &function.result_adapter {
            Some(adapter) => {
                let mut conversion = ConversionContext::in_native_call(self)?;
                let value = adapter.convert(&mut conversion, value)?;
                destination.publish(conversion.runtime_mut(), &value)
            }
            None => destination.publish(self.runtime, &value),
        }
    }
}
