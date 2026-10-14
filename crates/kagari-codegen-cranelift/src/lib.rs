//! Host-native compilation of verified MIR. Execution belongs to the runtime.
mod emit;
mod object;
mod scalar;
#[cfg(test)]
mod tests;

use cranelift_codegen::{
    isa::TargetIsa,
    settings::{self, Configurable, Flags},
};
use cranelift_native::builder_with_options;
use kagari_codegen::{
    BackendConfiguration, BackendFunctionInput, CodegenBackend,
    diagnostic::{BackendCompileError, BackendDiagnostic, BackendDiagnosticKind},
};
use std::{fmt, sync::Arc};
use {
    kagari_abi::native::{BackendId, BackendTarget},
    kagari_contract::native::NativeCompilationProduct,
};

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
        Self::from_isa(host_isa(true)?)
    }

    /// Baseline x86_64 Windows MSVC code, independent of build-host CPU extensions.
    pub fn for_aot() -> Result<Self, CraneliftBackendError> {
        if !cfg!(all(
            target_arch = "x86_64",
            target_os = "windows",
            target_env = "msvc"
        )) {
            return Err(CraneliftBackendError {
                message: "AOT currently requires x86_64 Windows MSVC".into(),
            });
        }
        let isa = host_isa(false)?;
        if isa
            .isa_flags()
            .iter()
            .any(|flag| flag.as_bool() != Some(false))
        {
            return Err(CraneliftBackendError {
                message: "unexpected non-baseline x64 ISA defaults".into(),
            });
        }
        let mut backend = Self::from_isa(isa)?;
        backend.configuration.options.push((
            "isa-settings".into(),
            backend.configuration.target.features.join(","),
        ));
        backend.configuration.target.features = vec!["sse2".into()];
        Ok(backend)
    }

    fn from_isa(isa: Arc<dyn TargetIsa>) -> Result<Self, CraneliftBackendError> {
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
// Helper requirements use the explicit ABI description and invocation context; compilation
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

fn host_isa(infer_native: bool) -> Result<Arc<dyn TargetIsa>, CraneliftBackendError> {
    let mut flags = settings::builder();
    for (name, value) in [("use_colocated_libcalls", "false"), ("is_pic", "true")] {
        flags
            .set(name, value)
            .map_err(|error| CraneliftBackendError {
                message: error.to_string(),
            })?;
    }
    builder_with_options(infer_native)
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
