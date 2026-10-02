use kagari_abi::native_api::NativeApiError;
use kagari_common::cancellation::Cancelled;

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("analysis cancelled")]
    Cancelled,
    #[error("invalid installed native API: {0}")]
    NativeApi(#[from] NativeApiError),
}

impl From<Cancelled> for AnalysisError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}
