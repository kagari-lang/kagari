use std::sync::Arc;

use cranelift_codegen::{
    isa::TargetIsa,
    settings::{self, Configurable},
};
use kagari_runtime::{
    BackendCompileError, BackendDiagnostic, BackendDiagnosticKind, BackendFunctionInput, BackendId,
    BackendTarget, CodegenBackend, ExecutableFunctionArtifact,
};

#[derive(Debug)]
pub struct CraneliftBackend {
    backend_id: BackendId,
    target: BackendTarget,
}

impl CraneliftBackend {
    pub fn for_host() -> Result<Self, CraneliftBackendError> {
        let isa = host_isa()?;
        Ok(Self {
            backend_id: BackendId::new("cranelift"),
            target: BackendTarget {
                triple: isa.triple().to_string(),
                pointer_width: isa.pointer_bytes() * 8,
                features: Vec::new(),
            },
        })
    }
}

impl CodegenBackend for CraneliftBackend {
    fn backend_id(&self) -> BackendId {
        self.backend_id.clone()
    }

    fn target(&self) -> BackendTarget {
        self.target.clone()
    }

    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<ExecutableFunctionArtifact, BackendCompileError> {
        Err(BackendCompileError {
            diagnostics: vec![BackendDiagnostic {
                kind: BackendDiagnosticKind::UnsupportedFunction,
                message: format!(
                    "Cranelift lowering for `{}` is not implemented yet",
                    input.function.name
                ),
            }],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraneliftBackendError {
    message: String,
}

impl CraneliftBackendError {
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for CraneliftBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CraneliftBackendError {}

fn host_isa() -> Result<Arc<dyn TargetIsa>, CraneliftBackendError> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("is_pic", "true")
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })?;
    let flags = settings::Flags::new(flag_builder);
    cranelift_native::builder()
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })?
        .finish(flags)
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_runtime::{
        BackendDiagnosticKind, BackendFunctionInput, LoadedModule, ModuleId,
        ReloadDependencySnapshot,
    };

    #[test]
    fn cranelift_backend_initializes_host_target_without_leaking_backend_types() {
        let backend =
            CraneliftBackend::for_host().expect("host Cranelift target should initialize");
        let target = backend.target();

        assert_eq!(backend.backend_id().as_str(), "cranelift");
        assert!(!target.triple.is_empty());
        assert!(matches!(target.pointer_width, 32 | 64));
    }

    #[test]
    fn cranelift_backend_reports_lowering_as_unsupported_until_baseline_compile_step() {
        let mut backend =
            CraneliftBackend::for_host().expect("host Cranelift target should initialize");
        let mut module = kagari_ir::bytecode::BytecodeModule::default();
        module
            .functions
            .push(kagari_ir::bytecode::BytecodeFunction {
                name: "main".to_owned(),
                ..kagari_ir::bytecode::BytecodeFunction::default()
            });
        let loaded = LoadedModule {
            id: ModuleId::new(0),
            name: "jit_test".to_owned(),
            epoch: kagari_runtime::reload::ModuleEpoch(0),
            bytecode: module,
        };
        let dependencies = ReloadDependencySnapshot::from_bytecode(&loaded.bytecode);

        let error = backend
            .compile_function(BackendFunctionInput {
                module_key: loaded.key(),
                module_name: &loaded.name,
                module: &loaded.bytecode,
                function: &loaded.bytecode.functions[0],
                dependencies,
            })
            .expect_err("M9.2 wires Cranelift but does not lower functions yet");

        assert_eq!(
            error.diagnostics[0].kind,
            BackendDiagnosticKind::UnsupportedFunction
        );
    }
}
