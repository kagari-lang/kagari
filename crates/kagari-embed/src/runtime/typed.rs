//! Host entry conversion delegates to the VM's common runtime conversion scope.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::KagariRuntime};
use kagari_runtime::{
    module::LoadedModule,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::PinnedFunction,
        typed::NativeContext,
    },
};
use kagari_vm::error::VmError;

impl KagariRuntime {
    /// Access retained objects and invoke prepared members under the caller's
    /// execution policy. The context cannot escape; returned handles retain their
    /// own values. Traps and unwinds release the execution session automatically.
    pub fn with_context<R>(
        &self,
        owner: &LoadedModule,
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
        &self,
        function: &PinnedFunction<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<R> {
        context.validate_for_execute("bound function")?;
        let mut cx = self
            .vm
            .context(function.owner())
            .map_err(EmbeddingError::vm)?;
        function
            .call_with_options(&mut cx, arguments, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))
    }

    pub fn execute_typed<A: IntoKagariArguments, R: FromKagari>(
        &self,
        module: &LoadedModule,
        entry: &str,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<R> {
        context.validate_for_execute(entry)?;
        let _session = self
            .vm
            .runtime()
            .begin_execution(module, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        self.vm
            .execute_typed(module, entry, arguments)
            .map_err(EmbeddingError::vm)
    }
}
