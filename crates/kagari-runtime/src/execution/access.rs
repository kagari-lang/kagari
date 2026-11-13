//! Host entry conversion uses Runtime's guarded native conversion scope.
use crate::{
    Runtime,
    context::ExecutionContext,
    module::{ExecutableModule, managed::source::RevisionSource},
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::Function,
        typed::NativeContext,
    },
    session::ExecutionEntry,
};

impl Runtime {
    /// Access retained objects and invoke prepared members under the caller's
    /// execution policy. The context cannot escape; returned handles retain their
    /// own values. Traps and unwinds release the execution session automatically.
    pub fn with_context<R>(
        &mut self,
        owner: &ExecutableModule,
        context: &ExecutionContext,
        access: impl FnOnce(&mut NativeContext<'_>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.with_context_source(owner, context, None, access)
    }

    /// A retained data handle supplies its actual layout source. Ordinary
    /// dependency/candidate admission still applies; this grants no callable
    /// exception for entering an unrelated retained execution graph.
    pub(super) fn with_context_source<R>(
        &mut self,
        owner: &ExecutableModule,
        context: &ExecutionContext,
        origin: Option<RevisionSource<'_>>,
        access: impl FnOnce(&mut NativeContext<'_>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        context.validate_for_execute("host object access")?;
        let origin = match origin {
            Some(origin) => origin,
            None => self.selection_origin(owner)?,
        };
        let mut session = self.begin_execution_inner(
            owner,
            context.runtime_options(),
            ExecutionEntry::Program,
            origin,
        )?;
        let runtime = session.runtime_mut();
        runtime.attach_execution_observer()?;
        let mut cx = runtime.context(owner)?;

        (|| {
            cx.poll()?;
            let result = access(&mut cx)?;
            cx.poll()?;
            Ok(result)
        })()
    }

    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
        function: &Function<A, R>,
        arguments: A,
    ) -> NativeResult<R> {
        let context = self.default_context.clone();
        self.call_with_context(function, arguments, &context)
    }

    pub fn call_with_context<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
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
