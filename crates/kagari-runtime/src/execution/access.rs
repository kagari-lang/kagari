//! Host entry conversion uses Runtime's guarded native conversion scope.
use crate::{
    Runtime,
    context::ExecutionContext,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::Function,
        typed::NativeContext,
    },
};

impl Runtime {
    /// Access retained objects and invoke prepared members under the caller's
    /// execution policy. The context cannot escape; returned handles retain their
    /// own values. Traps and unwinds release the execution session automatically.
    pub fn with_context<R>(
        &self,
        owner: &ExecutableModule,
        context: &ExecutionContext,
        access: impl FnOnce(&mut NativeContext<'_>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        context.validate_for_execute("host object access")?;
        let _session = self.begin_execution(owner, context.runtime_options())?;
        self.attach_execution_observer()?;
        let mut cx = self.context(owner)?;

        (|| {
            cx.poll()?;
            let result = access(&mut cx)?;
            cx.poll()?;
            Ok(result)
        })()
    }

    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &self,
        function: &Function<A, R>,
        arguments: A,
    ) -> NativeResult<R> {
        self.call_with_context(function, arguments, &self.default_context.clone())
    }

    pub fn call_with_context<A: IntoKagariArguments, R: FromKagari>(
        &self,
        function: &Function<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> NativeResult<R> {
        context.validate_for_execute("bound function")?;
        let prepared = function.prepare_call(self)?;
        let mut cx = self.context(prepared.owner())?;
        prepared.call_with_options(&mut cx, arguments, context.runtime_options())
    }
}
