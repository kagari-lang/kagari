//! Typed entry conversion uses checked semantic facts from the installed product.
use crate::{
    Runtime,
    context::ExecutionContext,
    error::{RuntimeError, fault::EngineFault, reason::ErrorReason},
    executor::{Executor, native::invoke_script},
    host_scope::scoped::HostScope,
    module::{ExecutableModule, InstalledProgram, entry::ProgramEntry},
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments, context::ConversionContext},
        typed::NativeContext,
    },
    session::ExecutionOptions,
    value::Value,
};

use std::slice;

impl Runtime {
    /// Execute a closed public entry using a branded external host scope. This
    /// uses the interpreter's ordinary checked entry, session and trap cleanup.
    pub fn execute_scoped<'entry>(
        scope: &HostScope<'_, '_>,
        program: &InstalledProgram,
        entry: impl Into<ProgramEntry<'entry>>,
        arguments: &[Value],
        context: &ExecutionContext,
    ) -> NativeResult<Value> {
        let entry = entry.into();
        context.validate_for_execute(entry.label())?;
        scope.execute(arguments, |runtime, arguments| {
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
                conversion.check_value(expected, value)?;
            }
            let _session = runtime.begin_execution(&owner, context.runtime_options())?;
            runtime.attach_execution_observer()?;
            Executor::new(runtime, &owner, function, arguments)
                .and_then(|mut executor| executor.run())
        })
    }

    /// Access and call typed handles with the interpreter's synchronous service.
    pub fn context<'a>(
        &'a self,
        owner: &'a ExecutableModule,
    ) -> Result<NativeContext<'a>, RuntimeError> {
        NativeContext::with_invoker(self, owner, invoke_script)
    }

    /// Execute a public path or explicit root entry with an owned typed result.
    /// Conversion uses the verified target's semantic signature and supplying module.
    pub fn execute_typed<'a, A: IntoKagariArguments, R: FromKagari>(
        &self,
        program: &InstalledProgram,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: A,
        context: &ExecutionContext,
    ) -> Result<R, RuntimeError> {
        let entry = entry.into();
        context.validate_for_execute(entry.label())?;
        let module = self.select_program(program)?;
        self.execute_typed_in_module(&module, entry, arguments, context.runtime_options())
    }

    #[cfg(test)]
    pub(crate) fn execute_module_typed<'a, A: IntoKagariArguments, R: FromKagari>(
        &self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: A,
    ) -> NativeResult<R> {
        self.execute_typed_in_module(module, entry, arguments, self.execution_options())
    }

    fn execute_typed_in_module<'a, A: IntoKagariArguments, R: FromKagari>(
        &self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        self.validate_executable_module(module)?;
        let entry = entry.into();
        let (owner, function) = entry.resolve(module).ok_or_else(|| {
            RuntimeError::new(ErrorReason::MissingFunction(entry.label().to_owned()))
        })?;
        let (owner, function) = self.select_function(&owner, function)?;
        let module = &owner;
        let body = module
            .bytecode
            .functions
            .get(function.index())
            .ok_or(RuntimeError::from(
                EngineFault::VerifiedFunctionReferenceIsMissing,
            ))?;
        let semantic = &body.metadata.semantic;
        let invalid =
            || RuntimeError::module_validation("typed entry requires a closed semantic signature");
        // A source-free boundary consumes an existing concrete entry. It cannot
        // infer generic arguments or synthesize missing operation witnesses.
        if semantic.generic.is_some() {
            return Err(invalid());
        }
        let params = (0..usize::from(body.parameter_count))
            .map(|index| semantic.params.get(&index).cloned().ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let result = semantic.result.as_ref().ok_or_else(invalid)?;
        let params = self.resolve_type_arguments(module, &params)?;
        let result = self
            .resolve_type_arguments(module, slice::from_ref(result))?
            .pop()
            .ok_or_else(invalid)?;
        let mut conversion = ConversionContext::new(self, module)?;
        conversion.check_type::<R>(&result)?;
        let _session = self.begin_interpreter_execution(module, options)?;
        let roots = arguments.into_arguments(&mut conversion, &params)?;
        let arguments = (0..params.len())
            .map(|slot| roots.get(self.gc(), slot).ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let value = Executor::new(self, module, function, &arguments)?.run()?;
        let rooted = self.root_value(value).ok_or_else(invalid)?;
        let value = rooted.value(self.gc()).ok_or_else(invalid)?;
        conversion.decode_value(&result, &value)
    }
}
