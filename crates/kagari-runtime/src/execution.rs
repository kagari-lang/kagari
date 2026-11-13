pub(crate) mod roots;
use crate::{
    Runtime,
    context::ExecutionContext,
    debug::DebugSession,
    error::{RuntimeError, reason::ErrorReason},
    error_trace::ResultFailure,
    executor::Executor,
    module::{ExecutableModule, InstalledProgram, entry::ProgramEntry},
    native::conversion::{arguments::ArgumentValues, context::ConversionContext},
    session::{ExecutionOptions, ExecutionSession, ExecutionTrace, SessionScope},
    value::carriers::RootedValue,
};

mod access;
pub mod call;
#[cfg(test)]
pub(crate) mod fixtures;
mod invocation;
mod managed;
mod members;
pub mod memory;
pub mod native;
pub mod owned;
pub mod reentry;
mod scheduler;
mod tasks;
mod typed;

use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use std::cell::{Ref, RefMut};
use {
    kagari_abi::native::BackendId,
    kagari_contract::{ids::FunctionRef, native::ExecutableFunctionArtifact},
};

#[derive(Debug)]
pub struct ExecutionReport {
    pub module_name: String,
    pub epoch: u64,
    pub entry: String,
    /// Owning retention transfers with this field and survives later calls/GC.
    /// Inspect through the owning runtime's heap; clones share the root lease.
    pub return_value: RootedValue,
    pub failure: Option<ResultFailure>,
    pub jit: Option<JitExecutionReport>,
    pub trace: Option<ExecutionTrace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JitExecutionReport {
    pub backend: BackendId,
    pub function: FunctionRef,
    pub status: JitExecutionStatus,
    pub artifact: Option<ExecutableFunctionArtifact>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitExecutionStatus {
    Native,
    InterpreterFallback,
}

impl Runtime {
    /// Set the policy used by typed host calls and submitted work.
    pub fn set_default_context(&mut self, context: ExecutionContext) {
        self.default_context = context;
    }

    pub fn default_context(&self) -> &ExecutionContext {
        &self.default_context
    }

    /// Execute a current installed public path or the explicit zero-argument root entry.
    pub fn execute<'a>(
        &mut self,
        program: &InstalledProgram,
        entry: impl Into<ProgramEntry<'a>>,
        args: &[RootedValue],
        context: &ExecutionContext,
    ) -> Result<ExecutionReport, RuntimeError> {
        let entry = entry.into();
        self.validate_entry_arguments(entry.label(), args)?;
        context.validate_for_execute(entry.label())?;
        let module = self.select_program(program)?;
        self.execute_module(&module, entry, context.runtime_options())
    }

    fn validate_entry_arguments(
        &self,
        entry: &str,
        args: &[RootedValue],
    ) -> Result<(), RuntimeError> {
        if !args.is_empty() {
            return Err(RuntimeError::new(ErrorReason::UnsupportedExecution(
                format!(
                    "entry `{entry}` received {} arguments, but argument passing is not implemented",
                    args.len()
                ),
            )));
        }
        Ok(())
    }

    pub fn attach_debug_session(&mut self, session: DebugSession) -> Result<(), RuntimeError> {
        self.set_execution_observer(session)?;
        Ok(())
    }

    pub fn debug_session(&self) -> Option<Ref<'_, DebugSession>> {
        self.execution_observer::<DebugSession>()
    }

    pub fn debug_session_mut(&mut self) -> Option<RefMut<'_, DebugSession>> {
        self.execution_observer_mut::<DebugSession>()
    }

    fn begin_interpreter_execution(
        &mut self,
        module: &ExecutableModule,
        options: ExecutionOptions,
    ) -> Result<ExecutionSession<'_>, RuntimeError> {
        let mut session = self.begin_execution(module, options)?;
        if session
            .runtime()
            .execution_observer::<DebugSession>()
            .is_some()
        {
            session.runtime_mut().attach_execution_observer()?;
        }
        Ok(session)
    }

    /// Execute from an explicitly supplied checked module with caller-selected inputs.
    /// Public paths and the verified root entry still govern access. Ordinary call
    /// selection remains current; candidate initialization inherits its active phase.
    /// This entry is useful for staged initialization and source-free backend drivers.
    pub fn execute_module<'a>(
        &mut self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        options: ExecutionOptions,
    ) -> Result<ExecutionReport, RuntimeError> {
        let entry = entry.into();
        self.validate_executable_module(module)?;
        let entry_name = entry.label().to_owned();
        let (module, function) = entry
            .resolve(module)
            .ok_or_else(|| RuntimeError::new(ErrorReason::MissingFunction(entry_name.clone())))?;
        let (module, function) = self.select_function(&module, function)?;
        let session = self.begin_interpreter_execution(&module, options)?;
        session
            .runtime
            .execute_selected(&module, function, &entry_name, &session.scope)
    }

    fn select_function(
        &self,
        module: &ExecutableModule,
        function: FunctionRef,
    ) -> Result<(ExecutableModule, FunctionRef), RuntimeError> {
        self.validate_executable_module(module)?;
        let route = module.bind_call_route(CallableTarget::Script(function))?;
        let selected = self.select_call_route(&route)?;
        let CallableTarget::Script(function) = selected.target() else {
            return Err(RuntimeError::module_validation(
                "script entry selected a native import",
            ));
        };
        Ok((selected.owner().clone(), function))
    }

    fn execute_selected(
        &mut self,
        module: &ExecutableModule,
        function: FunctionRef,
        label: &str,
        session: &SessionScope,
    ) -> Result<ExecutionReport, RuntimeError> {
        let return_value = Executor::new(self, module, function, ArgumentValues::scalars(&[]))?
            .run(|runtime, stack, value| stack.retain_result(runtime, value))?;
        let failure = self.result_failure(&return_value);

        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: label.to_owned(),
            failure,
            return_value,
            jit: None,
            trace: session.trace(self.resources()),
        })
    }

    /// Invokes a linked script implementation through a runtime-owned
    /// interface value. The method ID must belong to the value's retained definition
    /// prefix. The receiver and arguments remain rooted while the method body executes.
    pub fn invoke_interface_method(
        &mut self,
        interface: &RootedValue,
        method: &DefinitionId,
        arguments: &[RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        let resolved = self
            .resolve_interface_method(interface, method)?
            .admit(self)?;
        let loaded = resolved.implementation(self)?;
        let mut conversion = ConversionContext::new(self, &loaded)?;
        let args = conversion.root_arguments(arguments)?;
        Executor::new_interface(conversion.runtime_mut(), &resolved, args)?
            .run(|runtime, stack, value| stack.retain_result(runtime, value))
    }
}
