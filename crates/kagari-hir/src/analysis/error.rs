//! Failures of query preparation and conversion, separate from script diagnostics.

use kagari_common::{cancellation::Cancelled, identity::mapping::DefinitionMappingError};
use kagari_types::declaration::module::DeclarationError;

/// Failure to construct analysis; ordinary source errors are carried as diagnostics instead.
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    /// Cooperative cancellation interrupted preparation or identity mapping.
    #[error("analysis cancelled")]
    Cancelled,
    /// Scoped/portable definition metadata could not be mapped consistently.
    #[error("invalid analysis identity metadata: {0}")]
    Identity(DefinitionMappingError),
    /// Installed native declarations failed validation or import.
    #[error("invalid installed native API: {0}")]
    NativeApi(#[from] DeclarationError),
}

impl From<Cancelled> for AnalysisError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

impl From<DefinitionMappingError> for AnalysisError {
    fn from(error: DefinitionMappingError) -> Self {
        match error {
            DefinitionMappingError::Cancelled => Self::Cancelled,
            error => Self::Identity(error),
        }
    }
}
