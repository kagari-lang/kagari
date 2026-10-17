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

    pub fn is_unsupported(&self) -> bool {
        !self.diagnostics.is_empty()
            && self
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.kind == BackendDiagnosticKind::UnsupportedFunction)
    }
}
