use std::fmt;

use kagari_ir::bytecode::{BytecodeFunction, BytecodeModule, FunctionRef};

use crate::{ModuleKey, ReloadDependencySnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendId(String);

impl BackendId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BackendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendTarget {
    pub triple: String,
    pub pointer_width: u8,
    pub features: Vec<String>,
}

impl BackendTarget {
    pub fn new(triple: impl Into<String>, pointer_width: u8) -> Self {
        Self {
            triple: triple.into(),
            pointer_width,
            features: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackendFunctionInput<'a> {
    pub module_key: ModuleKey,
    pub module_name: &'a str,
    pub module: &'a BytecodeModule,
    pub function: &'a BytecodeFunction,
    pub dependencies: ReloadDependencySnapshot,
}

impl<'a> BackendFunctionInput<'a> {
    pub fn function_ref(&self) -> FunctionRef {
        self.function.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableEntryPoint {
    Unresolved,
    Symbol(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableSafepoint {
    pub instruction_offset: usize,
    pub live_value_slots: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableTrap {
    pub instruction_offset: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableFunctionArtifact {
    pub backend: BackendId,
    pub target: BackendTarget,
    pub function: FunctionRef,
    pub entry: ExecutableEntryPoint,
    pub safepoints: Vec<ExecutableSafepoint>,
    pub traps: Vec<ExecutableTrap>,
}

impl ExecutableFunctionArtifact {
    pub fn new(backend: BackendId, target: BackendTarget, function: FunctionRef) -> Self {
        Self {
            backend,
            target,
            function,
            entry: ExecutableEntryPoint::Unresolved,
            safepoints: Vec::new(),
            traps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendDiagnosticKind {
    UnsupportedFunction,
    InvalidInput,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDiagnostic {
    pub kind: BackendDiagnosticKind,
    pub message: String,
}

impl BackendDiagnostic {
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            kind: BackendDiagnosticKind::UnsupportedFunction,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCompileError {
    pub diagnostics: Vec<BackendDiagnostic>,
}

impl BackendCompileError {
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            diagnostics: vec![BackendDiagnostic::unsupported(message)],
        }
    }
}

pub trait CodegenBackend {
    fn backend_id(&self) -> BackendId;

    fn target(&self) -> BackendTarget;

    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<ExecutableFunctionArtifact, BackendCompileError>;
}
