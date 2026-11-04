//! Typed invocation keeps its Rust mapping until backend dispatch.

use crate::{
    error::RuntimeError,
    gc::roots::RootSet,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::ScriptCall,
        conversion::{
            FromKagari,
            arguments::{IntoKagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        function_handle::{PreparedCall, Target},
        typed::NativeContext,
    },
    session::ExecutionOptions,
    value::Value,
};
use std::marker::PhantomData;

impl<A, R> Clone for PreparedCall<A, R> {
    fn clone(&self) -> Self {
        Self {
            selection: self.selection.clone(),
            mapping: PhantomData,
        }
    }
}

impl<A, R> PreparedCall<A, R> {
    /// The exact supplying executable retained by this prepared call.
    pub fn owner(&self) -> &ExecutableModule {
        self.selection.owner()
    }
}

impl<A: IntoKagariArguments, R> PreparedCall<A, R> {
    pub(super) fn encode_arguments(
        &self,
        cx: &mut ConversionContext<'_>,
        arguments: A,
    ) -> NativeResult<(RootSet, Vec<Value>)> {
        let prepared = PreparedArguments::from_checked(&self.selection.signature.params);
        arguments
            .into_prepared_arguments(cx, &prepared)
            .map_err(|error| cx.runtime().resources().admit_execution_error(error))?
            .into_owned()
    }
}

impl<A: IntoKagariArguments, R: FromKagari> PreparedCall<A, R> {
    /// Invoke the exact selected body, preserving checked conversion and roots.
    ///
    /// Runtime admission, argument conversion, execution and result errors
    /// propagate normally. Return conversion failure preserves completed script
    /// effects; argument roots remain alive through that conversion.
    pub fn call_with_options(
        &self,
        cx: &mut NativeContext<'_>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        let function = &self.selection;
        let runtime = cx.runtime();
        function.validate(runtime)?;
        let invoke = cx.invoke_script.ok_or_else(|| {
            RuntimeError::module_validation("call context has no execution backend")
        })?;
        let _session = runtime.begin_pinned_execution_with_options(function, options)?;
        let mut conversion = ConversionContext::in_prepared_call(runtime, function)?;
        let prepared = PreparedArguments::from_checked(&function.signature.params);
        let mut slots = arguments
            .into_prepared_arguments(&mut conversion, &prepared)
            .map_err(|error| runtime.resources().admit_execution_error(error))?;
        let value = match &function.target {
            Target::Entry(target) if target.primitive.is_some() => runtime
                .invoke_standard_builtin(self.owner(), target.primitive.unwrap(), &slots.values()?)
                .map_err(|error| error.into_runtime_error())?,
            _ => invoke(
                runtime,
                self.owner(),
                ScriptCall::Pinned(function),
                slots.arguments(),
            )?,
        };
        slots.set(slots.result_index(), value)?;
        conversion.decode_result(&function.signature.result, &slots)
    }
}
