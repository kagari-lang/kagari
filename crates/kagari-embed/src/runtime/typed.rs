//! Host entry conversion delegates to the VM's common runtime conversion scope.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::KagariRuntime};
use kagari_runtime::{
    module::LoadedModule,
    native::{
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::PinnedFunction,
    },
};
use kagari_vm::error::VmError;

impl KagariRuntime {
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
