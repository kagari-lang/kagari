//! Owned diagnostic positions independent of Program and heap lifetimes.
use crate::{error_trace::ErrorTrace, task::TaskFailureOrigin};
use kagari_contract::standard::RuntimePrimitive;
use std::sync::Arc;

/// Positions are attached from inner to outer and displayed in reverse order.
/// Field/variant names are owned so diagnostics can outlive their Program.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorLocation {
    #[error("field {0}")]
    Field(String),
    #[error("variant {0}")]
    Variant(String),
    #[error("element {0}")]
    Element(usize),
    #[error("{0:?}")]
    Builtin(RuntimePrimitive),
}

impl ErrorLocation {
    pub(crate) fn retained_bytes(&self) -> usize {
        match self {
            Self::Field(name) | Self::Variant(name) => name.capacity(),
            Self::Element(_) | Self::Builtin(_) => 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ErrorContext {
    pub trace: Option<Arc<ErrorTrace>>,
    pub task_origin: Option<TaskFailureOrigin>,
    pub locations: Vec<ErrorLocation>,
}
