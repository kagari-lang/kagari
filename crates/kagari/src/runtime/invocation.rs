//! Typed async entrypoints share admission, scheduling and completion conversion.
use crate::{
    RunResult,
    context::ExecutionContext,
    runtime::{
        Runtime,
        call::{Call, PendingCall},
    },
};
use kagari_runtime::native::{
    conversion::{FromKagari, arguments::IntoKagariArguments},
    function_handle::asynchronous::AsyncFunction,
    interfaces::{Interface, asynchronous::AsyncInterfaceMethod},
    methods::asynchronous::AsyncMethod,
    objects::Object,
};

impl Runtime {
    /// Admit a cold pinned invocation using the default context.
    /// Arguments convert and become rooted before return; neither factory nor
    /// async body runs here. Poll Runtime separately to make progress. Admission
    /// errors return now; execution/conversion errors arrive through the ticket.
    pub fn submit_async<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        function: &AsyncFunction<A, R>,
        arguments: A,
    ) -> RunResult<PendingCall<R>> {
        self.submit_async_with_context(function, arguments, &self.default_context.clone())
    }

    /// Submit with explicit cancellation, deterministic inputs and interpreter policy.
    /// Invocation cancellation does not cancel the supplied shared context token.
    pub fn submit_async_with_context<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        function: &AsyncFunction<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<PendingCall<R>> {
        context.validate_for_execute("async function")?;
        Ok(PendingCall {
            result: function.schedule(self.runtime(), arguments, context.runtime_options())?,
        })
    }

    /// Admit a cold invocation and borrow this runtime until completion or drop.
    /// Awaiting drives bounded shared scheduler turns; no external pump is needed.
    /// Dropping the Call synchronously cleans up its invocation and descendants.
    pub fn call_async<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        function: &AsyncFunction<A, R>,
        arguments: A,
    ) -> RunResult<Call<'_, R>> {
        let pending = self.submit_async(function, arguments)?;
        Ok(Call::new(self, pending))
    }

    /// Direct-await admission with an explicit context, retained through conversion.
    pub fn call_async_with_context<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        function: &AsyncFunction<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<Call<'_, R>> {
        let pending = self.submit_async_with_context(function, arguments, context)?;
        Ok(Call::new(self, pending))
    }

    /// Submit a Future-returning inherent method; arguments exclude `self`.
    /// Retains the checked receiver and rejects a foreign or incompatible generation.
    pub fn submit_async_method<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        object: &Object<impl Sized>,
        method: &AsyncMethod<A, R>,
        arguments: A,
    ) -> RunResult<PendingCall<R>> {
        self.submit_async_method_with_context(
            object,
            method,
            arguments,
            &self.default_context.clone(),
        )
    }

    pub fn submit_async_method_with_context<
        A: IntoKagariArguments,
        R: FromKagari + Send + 'static,
    >(
        &mut self,
        object: &Object<impl Sized>,
        method: &AsyncMethod<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<PendingCall<R>> {
        context.validate_for_execute("async method")?;
        Ok(PendingCall {
            result: object.schedule_async(
                self.runtime(),
                method,
                arguments,
                context.runtime_options(),
            )?,
        })
    }

    /// Directly await an inherent method through the common runtime driver.
    pub fn call_async_method<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        object: &Object<impl Sized>,
        method: &AsyncMethod<A, R>,
        arguments: A,
    ) -> RunResult<Call<'_, R>> {
        let pending = self.submit_async_method(object, method, arguments)?;
        Ok(Call::new(self, pending))
    }

    pub fn call_async_method_with_context<
        A: IntoKagariArguments,
        R: FromKagari + Send + 'static,
    >(
        &mut self,
        object: &Object<impl Sized>,
        method: &AsyncMethod<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<Call<'_, R>> {
        let pending = self.submit_async_method_with_context(object, method, arguments, context)?;
        Ok(Call::new(self, pending))
    }

    /// Submit through an existing retained interface and its checked member binding.
    /// Dispatch and output adapters remain pinned; this does not cast struct objects.
    pub fn submit_async_interface_method<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        object: &Interface<impl Sized>,
        method: &AsyncInterfaceMethod<A, R>,
        arguments: A,
    ) -> RunResult<PendingCall<R>> {
        self.submit_async_interface_method_with_context(
            object,
            method,
            arguments,
            &self.default_context.clone(),
        )
    }

    pub fn submit_async_interface_method_with_context<
        A: IntoKagariArguments,
        R: FromKagari + Send + 'static,
    >(
        &mut self,
        object: &Interface<impl Sized>,
        method: &AsyncInterfaceMethod<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<PendingCall<R>> {
        context.validate_for_execute("async interface method")?;
        Ok(PendingCall {
            result: object.schedule_async(
                self.runtime(),
                method,
                arguments,
                context.runtime_options(),
            )?,
        })
    }

    /// Directly await a Future-returning interface member with shared scheduling.
    pub fn call_async_interface_method<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &mut self,
        object: &Interface<impl Sized>,
        method: &AsyncInterfaceMethod<A, R>,
        arguments: A,
    ) -> RunResult<Call<'_, R>> {
        let pending = self.submit_async_interface_method(object, method, arguments)?;
        Ok(Call::new(self, pending))
    }

    pub fn call_async_interface_method_with_context<
        A: IntoKagariArguments,
        R: FromKagari + Send + 'static,
    >(
        &mut self,
        object: &Interface<impl Sized>,
        method: &AsyncInterfaceMethod<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<Call<'_, R>> {
        let pending =
            self.submit_async_interface_method_with_context(object, method, arguments, context)?;
        Ok(Call::new(self, pending))
    }
}
