//! Typed entry conversion uses checked semantic facts from the installed product.
use crate::{
    error::VmError,
    executor::{
        Executor,
        native::{INVOKE_SCRIPT, native_error},
    },
    vm::Vm,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    host_scope::{scoped::HostScope, value::ScopedValue},
    module::{ExecutableModule, Program, entry::ProgramEntry},
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments, context::ConversionContext},
        function_handle::Function,
        typed::NativeContext,
    },
    session::{ExecutionOptions, ExecutionRetention},
    value::Value,
};
use std::slice;

impl Vm {
    /// Execute a closed public entry using a branded external host scope. This
    /// uses the interpreter's ordinary checked entry, session and trap cleanup.
    pub fn execute_scoped<'entry, 'scope>(
        scope: &HostScope<'scope, '_>,
        program: &Program,
        entry: impl Into<ProgramEntry<'entry>>,
        arguments: &[ScopedValue<'scope>],
        options: ExecutionOptions,
    ) -> NativeResult<ScopedValue<'scope>> {
        let entry = entry.into();
        let invoke = |runtime: &Runtime, arguments: &[Value]| {
            let module = runtime.select_program(program)?;
            let (owner, function) = entry
                .resolve(&module)
                .ok_or_else(|| RuntimeError::module_validation("missing scoped entry"))?;
            runtime.validate_executable_module(&owner)?;
            let body = &owner.bytecode.functions[function.index()];
            let semantic = &body.metadata.semantic;
            if semantic.generic.is_some() || arguments.len() != usize::from(body.parameter_count) {
                return Err(RuntimeError::module_validation(
                    "scoped entry requires a closed signature and exact argument count",
                ));
            }
            let parameters = (0..arguments.len())
                .map(|index| {
                    semantic.params.get(&index).cloned().ok_or_else(|| {
                        RuntimeError::module_validation("missing scoped parameter type")
                    })
                })
                .collect::<NativeResult<Vec<_>>>()?;
            let parameters = runtime.resolve_type_arguments(&owner, &parameters)?;
            let conversion = ConversionContext::new(runtime, &owner)?;
            for (expected, value) in parameters.iter().zip(arguments) {
                // SAFETY: HostScope::execute_transport retained the complete argument pack
                // before entering this closure; the originating host scope is active.
                unsafe { conversion.check_transport(expected, value) }?;
            }
            let _session = runtime.begin_execution(&owner, options)?;
            runtime.attach_execution_observer()?;
            Executor::new(runtime, &owner, function, arguments)
                .and_then(|mut executor| executor.run())
                .map_err(native_error)
        };
        // SAFETY: The admitted argument pack stays rooted through Executor frame
        // publication. Its terminal result is immediately rooted by the scope.
        unsafe { scope.execute_transport(arguments, invoke) }
    }

    /// Access and call typed handles with the interpreter's synchronous service.
    pub fn context<'a>(
        &'a self,
        owner: &'a ExecutableModule,
    ) -> Result<NativeContext<'a>, VmError> {
        NativeContext::with_invoker(&self.runtime, owner, INVOKE_SCRIPT).map_err(VmError::from)
    }

    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &self,
        function: &Function<A, R>,
        arguments: A,
    ) -> Result<R, VmError> {
        let prepared = function.prepare_call(&self.runtime)?;
        let mut cx = self.context(prepared.owner())?;
        prepared
            .call_with_options(&mut cx, arguments, self.runtime.execution_options())
            .map_err(VmError::from)
    }

    /// Execute a public path or explicit root entry with an owned typed result.
    /// Conversion uses the verified target's semantic signature and supplying module.
    pub fn execute_typed<'a, A: IntoKagariArguments, R: FromKagari>(
        &self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: A,
    ) -> Result<R, VmError> {
        self.execute_typed_inner(module, entry, arguments, None)
    }

    /// Execute a typed entry while borrowing an existing exact execution retention.
    /// Uncovered selected owners use ordinary conversion retention. This does not
    /// start a session early or bypass entry, conversion or publication admission.
    pub fn execute_typed_in_execution<'a, A: IntoKagariArguments, R: FromKagari>(
        &self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: A,
        authority: &ExecutionRetention<'_>,
    ) -> Result<R, VmError> {
        self.execute_typed_inner(module, entry, arguments, Some(authority))
    }

    fn execute_typed_inner<'a, A: IntoKagariArguments, R: FromKagari>(
        &self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: A,
        authority: Option<&ExecutionRetention<'_>>,
    ) -> Result<R, VmError> {
        self.runtime.validate_executable_module(module)?;
        let entry = entry.into();
        let (owner, function) = entry
            .resolve(module)
            .ok_or_else(|| VmError::MissingFunction(entry.label().to_owned()))?;
        let (owner, function) = self.select_function(&owner, function)?;
        let module = &owner;
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
        let mut conversion = match authority {
            Some(authority) => ConversionContext::in_execution(&self.runtime, module, authority)?,
            None => ConversionContext::new(&self.runtime, module)?,
        };
        conversion.check_type::<R>(&result)?;
        let _session = self.begin_execution(module)?;
        let roots = arguments.into_arguments(&mut conversion, &params)?;
        // SAFETY: The private argument pack stays live until Executor publishes
        // every input to its frame; no caller callback runs during that handoff.
        let arguments = (0..params.len())
            .map(|slot| unsafe { roots.get_transport(self.runtime.gc(), slot) }.ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let value = Executor::new(&self.runtime, module, function, &arguments)?.run()?;
        let rooted = unsafe {
            // SAFETY: Publish the terminal result before any user result conversion can run or collect.
            self.runtime.gc().root_value_transport(value)
        }
        .ok_or_else(invalid)?;
        let value = conversion.retain(&result, rooted)?;
        conversion
            .decode_value(&result, &value)
            .map_err(VmError::from)
    }
}
