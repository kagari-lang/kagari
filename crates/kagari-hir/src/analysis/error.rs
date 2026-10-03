use kagari_common::{cancellation::Cancelled, identity::mapping::DefinitionMappingError};
use kagari_contract::declaration::DeclarationError;

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("analysis cancelled")]
    Cancelled,
    #[error("invalid analysis identity metadata: {0}")]
    Identity(DefinitionMappingError),
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
