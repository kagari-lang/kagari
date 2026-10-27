//! Host entry conversion delegates to the VM's common runtime conversion scope.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::Runtime};
use kagari_runtime::{
    module::{ExecutableModule, Program, entry::ProgramEntry},
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::Function,
        typed::NativeContext,
    },
};
use kagari_vm::error::VmError;

impl Runtime {
    /// Access retained objects and invoke prepared members under the caller's
    /// execution policy. The context cannot escape; returned handles retain their
    /// own values. Traps and unwinds release the execution session automatically.
    pub fn with_context<R>(
        &self,
        owner: &ExecutableModule,
        context: &ExecutionContext,
        access: impl FnOnce(&mut NativeContext<'_>) -> NativeResult<R>,
    ) -> RunResult<R> {
        context.validate_for_execute("host object access")?;
        let _session = self
            .vm
            .runtime()
            .begin_execution(owner, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        self.vm
            .runtime()
            .attach_execution_observer()
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        let mut cx = self.vm.context(owner).map_err(EmbeddingError::vm)?;
        let result = (|| {
            cx.poll()?;
            let result = access(&mut cx)?;
            cx.poll()?;
            Ok(result)
        })();
        result.map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))
    }

    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
        function: &Function<A, R>,
        arguments: A,
    ) -> RunResult<R> {
        self.call_with_context(function, arguments, &self.default_context.clone())
    }

    pub fn call_with_context<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
        function: &Function<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<R> {
        context.validate_for_execute("bound function")?;
        let prepared = function.prepare_call(self.runtime())?;
        let mut cx = self
            .vm
            .context(prepared.owner())
            .map_err(EmbeddingError::vm)?;
        prepared
            .call_with_options(&mut cx, arguments, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))
    }

    pub fn execute_typed<'a, A: IntoKagariArguments, R: FromKagari>(
        &self,
        program: &Program,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<R> {
        let entry = entry.into();
        context.validate_for_execute(entry.label())?;
        let module = self.runtime().select_program(program)?;
        let session = self
            .vm
            .runtime()
            .begin_execution(&module, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        self.vm
            .execute_typed_in_execution(&module, entry, arguments, &session.retention())
            .map_err(EmbeddingError::vm)
    }
}
