//! Execute a preparation decision supplied before script entry. Compilation belongs to the SDK.
use kagari_abi::native::BackendId;
use kagari_runtime::{
    backend::{BackendInvocationError, native::InstalledNativeFunction},
    error::RuntimeError,
    module::LoadedModule,
};

use crate::{
    error::VmError,
    executor::Executor,
    vm::{ExecutionReport, JitExecutionReport, JitExecutionStatus, Vm, find_function_ref},
};

/// A preparation result, with no compiler or MIR types in the VM contract.
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

impl Vm {
    /// Execute already-prepared code or an explicit pre-entry fallback decision.
    /// This method never compiles and never restarts a partially executed native call.
    pub fn execute_prepared(
        &mut self,
        module: &LoadedModule,
        entry: &str,
        prepared: &PreparedNativeEntry,
    ) -> Result<ExecutionReport, VmError> {
        let session = self.begin_execution(module)?;
        self.runtime.validate_loaded_module(module)?;
        let function = find_function_ref(&module.bytecode, entry)?;
        let (backend, artifact, mut diagnostics) = match prepared {
            PreparedNativeEntry::Native(installed) => {
                self.runtime.validate_loaded_module(installed.module())?;
                if installed.module().key() != module.key()
                    || installed.module().program_fingerprint() != module.program_fingerprint()
                    || installed.artifact().function != function
                {
                    return Err(VmError::RuntimeError(RuntimeError::module_validation(
                        "prepared native entry belongs to a different function or execution version",
                    )));
                }
                (
                    installed.artifact().backend.clone(),
                    Some(installed.artifact().clone()),
                    Vec::new(),
                )
            }
            PreparedNativeEntry::Unsupported {
                backend,
                diagnostics,
            } => (backend.clone(), None, diagnostics.clone()),
        };
        let native = match prepared {
            PreparedNativeEntry::Native(installed) => {
                if let Err(error) = self.runtime.validate_jit_boundary() {
                    diagnostics.push(format!("JIT disabled by runtime policy: {error}"));
                    None
                } else {
                    match self.runtime.invoke_native_function(installed) {
                        Ok(value) => Some(value),
                        Err(failure) => match failure.error {
                            // Runtime promises this variant only before entering native code.
                            BackendInvocationError::UnsupportedArtifact(reason) => {
                                diagnostics.push(reason);
                                None
                            }
                            BackendInvocationError::RuntimeFailure(error) => {
                                return Err(VmError::RuntimeError(error).with_trace(failure.trace));
                            }
                            error => {
                                return Err(VmError::JitInvocation(error).with_trace(failure.trace));
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
                Executor::new(&self.runtime, module, function, &[])?.run()?,
                JitExecutionStatus::InterpreterFallback,
                None,
            ),
        };
        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: entry.to_owned(),
            failure: self.runtime.result_failure(&return_value),
            return_value,
            jit: Some(JitExecutionReport {
                backend,
                function,
                status,
                artifact,
                diagnostics,
            }),
            trace: session.trace(),
        })
    }
}
