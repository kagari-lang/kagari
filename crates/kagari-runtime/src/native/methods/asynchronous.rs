//! Async inherent members reuse receiver validation and current-call admission.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::roots::RootedValue,
    module::dispatch::CallRoute,
    native::{
        binding::NativeResult,
        conversion::{
            FromKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments},
        },
        function_handle::asynchronous::AsyncFunction,
        methods::{
            InherentType,
            arguments::ReceiverArguments,
            receiver::{AppliedReceiver, RetainedReceiver},
        },
    },
    scheduler::result::ScheduledResult,
    session::ExecutionOptions,
    source::FunctionSourceTarget,
};

/// A current-call Future-returning inherent method; `A` excludes its receiver.
/// Receiver compatibility is checked again at admission before argument conversion.
#[derive(Debug)]
pub struct AsyncMethod<A, R> {
    receiver: AppliedReceiver,
    function: AsyncFunction<ReceiverArguments<A>, R>,
}

impl<A, R> Clone for AsyncMethod<A, R> {
    fn clone(&self) -> Self {
        Self {
            receiver: self.receiver.clone(),
            function: self.function.clone(),
        }
    }
}

impl Runtime {
    pub fn bind_async_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        object_type: &impl InherentType,
        name: &str,
    ) -> NativeResult<AsyncMethod<A, R>> {
        let method = self.bind_method(object_type, name)?;
        Ok(AsyncMethod {
            receiver: method.receiver,
            function: AsyncFunction::from_function(self, method.function)?,
        })
    }

    pub fn bind_async_associated_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        object_type: &impl InherentType,
        name: &str,
    ) -> NativeResult<AsyncFunction<A, R>> {
        AsyncFunction::from_function(self, self.bind_associated_function(object_type, name)?)
    }
}

impl<A, R> AsyncMethod<A, R> {
    pub fn source_target(&self) -> FunctionSourceTarget {
        self.function.source_target()
    }

    pub fn call_route(&self) -> Option<&CallRoute> {
        self.function.call_route()
    }
}

impl<A: IntoKagariArguments, R: FromKagari + Send + 'static> AsyncMethod<A, R> {
    pub(crate) fn schedule_on(
        &self,
        runtime: &Runtime,
        receiver: AppliedReceiver,
        root: RootedValue,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<ScheduledResult<R>> {
        self.receiver.validate(runtime)?;
        receiver.validate(runtime)?;
        if !self.receiver.matches(&receiver) {
            return Err(RuntimeError::module_validation(
                "method binding belongs to another applied type or installation",
            ));
        }
        self.function.schedule(
            runtime,
            ReceiverArguments {
                receiver: RetainedReceiver {
                    root,
                    applied: receiver,
                },
                arguments,
            },
            options,
        )
    }
}
