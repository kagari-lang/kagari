//! Host-native compilation of verified MIR. Execution belongs to the runtime.
mod emit;
mod scalar;
#[cfg(test)]
mod tests;

use cranelift_codegen::{
    isa::TargetIsa,
    settings::{self, Configurable, Flags},
};
use cranelift_native::builder as native_builder;
use kagari_abi::native::{BackendId, BackendTarget, NativeCompilationProduct};
use kagari_codegen::{
    BackendConfiguration, BackendFunctionInput, CodegenBackend,
    diagnostic::{BackendCompileError, BackendDiagnostic, BackendDiagnosticKind},
};
use std::{fmt, sync::Arc};

pub struct CraneliftBackend {
    isa: Arc<dyn TargetIsa>,
    configuration: BackendConfiguration,
}

impl fmt::Debug for CraneliftBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CraneliftBackend")
            .field("configuration", &self.configuration)
            .finish_non_exhaustive()
    }
}

impl CraneliftBackend {
    pub fn for_host() -> Result<Self, CraneliftBackendError> {
        let isa = host_isa()?;
        let mut features = isa
            .isa_flags()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        features.sort();
        let configuration = BackendConfiguration {
            backend: BackendId::new("cranelift"),
            target: BackendTarget {
                triple: isa.triple().to_string(),
                pointer_width: isa.pointer_bytes() * 8,
                features,
            },
            options: vec![
                (
                    "cranelift-version".into(),
                    cranelift_codegen::VERSION.into(),
                ),
                ("settings".into(), isa.flags().to_string()),
            ],
        };
        Ok(Self { isa, configuration })
    }
}

// SAFETY: emission accepts only the verified straight-line scalar subset, uses
// sealed logical offsets for every helper boundary, and implements the native ABI.
// Each product owns a finalized module and its pages independently of this backend.
// Helpers are taken from the caller's explicit ABI link description; compilation
// invokes no script code. Configuration includes the host ISA and all codegen flags.
unsafe impl CodegenBackend for CraneliftBackend {
    fn configuration(&self) -> BackendConfiguration {
        self.configuration.clone()
    }
    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<NativeCompilationProduct, BackendCompileError> {
        emit::compile(self.isa.clone(), &self.configuration, input)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct CraneliftBackendError {
    message: String,
}
impl CraneliftBackendError {
    pub fn message(&self) -> &str {
        &self.message
    }
}

fn host_isa() -> Result<Arc<dyn TargetIsa>, CraneliftBackendError> {
    let mut flags = settings::builder();
    for (name, value) in [("use_colocated_libcalls", "false"), ("is_pic", "false")] {
        flags
            .set(name, value)
            .map_err(|error| CraneliftBackendError {
                message: error.to_string(),
            })?;
    }
    native_builder()
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })?
        .finish(Flags::new(flags))
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })
}

fn internal_error(message: impl Into<String>) -> BackendCompileError {
    BackendCompileError {
        diagnostics: vec![BackendDiagnostic {
            kind: BackendDiagnosticKind::InternalError,
            message: message.into(),
        }],
    }
}
