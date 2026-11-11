//! Execute a preparation decision supplied before script entry. Compilation belongs to the SDK.
use crate::{
    Runtime,
    backend::{BackendInvocationError, native::InstalledNativeFunction},
    context::ExecutionContext,
    error::{RuntimeError, reason::ErrorReason},
    execution::{ExecutionReport, JitExecutionReport, JitExecutionStatus},
    executor::Executor,
    module::{ExecutableModule, InstalledProgram, entry::ProgramEntry},
    session::{ExecutionOptions, SessionScope},
    value::Value,
};

use kagari_abi::native::BackendId;
use kagari_contract::ids::FunctionRef;

/// A preparation result, with no compiler or MIR types in the Runtime contract.
/// Compilation failures other than unsupported input must be handled by the caller,
/// rather than converted to an interpreter fallback.
#[derive(Debug, Clone)]
pub enum PreparedNativeEntry {
    Native(InstalledNativeFunction),
    Unsupported {
        backend: BackendId,
        diagnostics: Vec<String>,
    },
}

/// A prepared entry and an optional host request to interpret this invocation.
/// Runtime validates the original native handle before honoring `fallback_reason`,
/// and includes that reason in the interpreter fallback report.
#[derive(Debug, Clone, Copy)]
pub struct PreparedNativeExecution<'a> {
    pub prepared: &'a PreparedNativeEntry,
    pub fallback_reason: Option<&'a str>,
}

impl<'a> From<&'a PreparedNativeEntry> for PreparedNativeExecution<'a> {
    fn from(prepared: &'a PreparedNativeEntry) -> Self {
        Self {
            prepared,
            fallback_reason: None,
        }
    }
}

impl Runtime {
    /// Execute already-prepared code or an explicit pre-entry fallback decision.
    /// This method never compiles and never restarts a partially executed native call.
    pub fn execute_prepared<'a>(
        &mut self,
        program: &InstalledProgram,
        entry: impl Into<ProgramEntry<'a>>,
        args: &[Value],
        context: &ExecutionContext,
        prepared: &PreparedNativeEntry,
    ) -> Result<ExecutionReport, RuntimeError> {
        let entry = entry.into();
        self.validate_entry_arguments(entry.label(), args)?;
        let module = self.select_program(program)?;
        self.execute_prepared_module(
            &module,
            entry,
            PreparedNativeExecution {
                prepared,
                fallback_reason: context.jit_policy.fallback_reason(),
            },
            context.runtime_options(),
        )
    }

    pub fn execute_prepared_module<'a, 'p>(
        &mut self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        prepared: impl Into<PreparedNativeExecution<'p>>,
        options: ExecutionOptions,
    ) -> Result<ExecutionReport, RuntimeError> {
        self.validate_executable_module(module)?;
        let entry = entry.into();
        let (owner, function) = entry
            .resolve(module)
            .ok_or_else(|| RuntimeError::new(ErrorReason::MissingFunction(entry.label().into())))?;
        let (owner, function) = self.select_function(&owner, function)?;
        let session = self.begin_interpreter_execution(&owner, options)?;
        session.runtime.execute_prepared_selected(
            &owner,
            function,
            entry.label(),
            prepared,
            &session.scope,
        )
    }

    pub(super) fn execute_prepared_selected<'p>(
        &mut self,
        module: &ExecutableModule,
        function: FunctionRef,
        label: &str,
        prepared: impl Into<PreparedNativeExecution<'p>>,
        session: &SessionScope,
    ) -> Result<ExecutionReport, RuntimeError> {
        let PreparedNativeExecution {
            prepared,
            fallback_reason,
        } = prepared.into();
        let (backend, artifact, mut diagnostics) = match prepared {
            PreparedNativeEntry::Native(installed) => {
                self.validate_executable_module(installed.module())?;
                if installed.module().key() != module.key()
                    || installed.module().program_fingerprint() != module.program_fingerprint()
                    || installed.artifact().function != function
                {
                    return Err(RuntimeError::module_validation(
                        "prepared native entry belongs to a different function or execution version",
                    ));
                }
                (
                    installed.artifact().code.backend.clone(),
                    Some(installed.artifact().clone()),
                    Vec::new(),
                )
            }
            PreparedNativeEntry::Unsupported {
                backend,
                diagnostics,
            } => (backend.clone(), None, diagnostics.clone()),
        };
        if let Some(reason) = fallback_reason {
            diagnostics.push(reason.to_owned());
        }
        let native = match prepared {
            PreparedNativeEntry::Native(_) if fallback_reason.is_some() => None,
            PreparedNativeEntry::Native(installed) => {
                if let Err(error) = self.resources().ensure_execution_allowed() {
                    diagnostics.push(format!("JIT disabled by runtime policy: {error}"));
                    None
                } else {
                    match self.invoke_native_function(installed) {
                        Ok(value) => Some(value),
                        Err(failure) => match failure.error {
                            // Runtime promises this variant only before entering native code.
                            BackendInvocationError::UnsupportedArtifact(reason) => {
                                diagnostics.push(reason);
                                None
                            }
                            BackendInvocationError::RuntimeFailure(error) => {
                                return Err(error.with_trace(failure.trace));
                            }
                            error => {
                                return Err(RuntimeError::from(error).with_trace(failure.trace));
                            }
                        },
                    }
                }
            }
            PreparedNativeEntry::Unsupported { .. } => None,
        };
        let (return_value, status, artifact) = match native {
            Some(value) => (value, JitExecutionStatus::Native, artifact),
            None => (
                Executor::new(self, module, function, &[])?.run()?,
                JitExecutionStatus::InterpreterFallback,
                None,
            ),
        };
        let failure = self.result_failure(&return_value);
        let return_value = self
            .root_value(return_value)
            .ok_or_else(|| RuntimeError::module_validation("execution result root"))?;
        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: label.to_owned(),
            failure,
            return_value,
            jit: Some(JitExecutionReport {
                backend,
                function,
                status,
                artifact,
                diagnostics,
            }),
            trace: session.trace(self.resources()),
        })
    }
}
