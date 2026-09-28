use kagari_common::cancellation::Cancelled;
use kagari_stdlib::PackageError;

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("analysis cancelled")]
    Cancelled,
    #[error("invalid installed standard library: {0}")]
    StandardLibrary(PackageError),
}

impl From<Cancelled> for AnalysisError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

impl From<PackageError> for AnalysisError {
    fn from(error: PackageError) -> Self {
        match error {
            PackageError::Cancelled => Self::Cancelled,
            error => Self::StandardLibrary(error),
        }
    }
}
