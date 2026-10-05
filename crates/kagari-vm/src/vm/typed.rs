//! Typed entry conversion uses checked semantic facts from the installed product.
use crate::{
    error::VmError,
    executor::{Executor, native::invoke_script},
    vm::{Vm, find_function_ref},
};
use kagari_runtime::{
    error::RuntimeError,
    module::LoadedModule,
    native::{
        conversion::{FromKagari, arguments::IntoKagariArguments, context::ConversionContext},
        function_handle::PinnedFunction,
        typed::NativeContext,
    },
};
use std::slice;

impl Vm {
    /// Access and call typed handles with the interpreter's synchronous service.
    pub fn context<'a>(&'a self, owner: &'a LoadedModule) -> Result<NativeContext<'a>, VmError> {
        NativeContext::with_invoker(&self.runtime, owner, invoke_script).map_err(VmError::from)
    }

    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &self,
        function: &PinnedFunction<A, R>,
        arguments: A,
    ) -> Result<R, VmError> {
        let mut cx = self.context(function.owner())?;
        function.call(&mut cx, arguments).map_err(VmError::from)
    }

    /// Execute a named entry with one outer argument tuple and an owned result.
    /// This uses the same explicit entry policy as `execute`. Cached public
    /// function/member binding is a separate, visibility-checked operation.
    pub fn execute_typed<A: IntoKagariArguments, R: FromKagari>(
        &self,
        module: &LoadedModule,
        entry: &str,
        arguments: A,
    ) -> Result<R, VmError> {
        self.runtime.validate_loaded_module(module)?;
        let function = find_function_ref(&module.bytecode, entry)?;
        let body = module
            .bytecode
            .functions
            .get(function.index())
            .ok_or(VmError::InvalidFunctionRef(function))?;
        let semantic = &body.metadata.semantic;
        let invalid =
            || RuntimeError::module_validation("typed entry requires a closed semantic signature");
        // A source-free boundary consumes an existing concrete entry. It cannot
        // infer generic arguments or synthesize missing operation witnesses.
        if semantic.generic.is_some() {
            return Err(invalid().into());
        }
        let params = (0..usize::from(body.parameter_count))
            .map(|index| semantic.params.get(&index).cloned().ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let result = semantic.result.as_ref().ok_or_else(invalid)?;
        let params = self.runtime.resolve_type_arguments(module, &params)?;
        let result = self
            .runtime
            .resolve_type_arguments(module, slice::from_ref(result))?
            .pop()
            .ok_or_else(invalid)?;
        let mut conversion = ConversionContext::new(&self.runtime, module)?;
        conversion.check_type::<R>(&result)?;
        let _session = self.begin_execution(module)?;
        let roots = arguments.into_arguments(&mut conversion, &params)?;
        let arguments = (0..params.len())
            .map(|slot| roots.get(self.runtime.gc(), slot).ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let value = Executor::new(&self.runtime, module, function, &arguments)?.run()?;
        let rooted = self.runtime.root_value(value).ok_or_else(invalid)?;
        let value = rooted.value(self.runtime.gc()).ok_or_else(invalid)?;
        conversion
            .decode_value(&result, &value)
            .map_err(VmError::from)
    }
}
