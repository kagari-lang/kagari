use kagari_common::cancellation::Cancelled;
use std::ops::Range;
use thiserror::Error;

/// A failed package input operation, with its manifest or source origin retained.
#[derive(Debug, Error)]
pub enum PackageError {
    #[error("{origin}: {message}")]
    Input {
        origin: String,
        /// Byte offsets in the input, when supplied by its parser.
        range: Option<Range<usize>>,
        message: String,
    },
    #[error("package input operation cancelled")]
    Cancelled,
}

impl PackageError {
    pub(crate) fn input(origin: &str, message: impl Into<String>) -> Self {
        Self::Input {
            origin: origin.into(),
            range: None,
            message: message.into(),
        }
    }
}

impl From<Cancelled> for PackageError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}
