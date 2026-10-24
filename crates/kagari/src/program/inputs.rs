//! Immutable preparation-side inputs retained by exact runtime publication token.

#[cfg(feature = "source")]
use crate::engine::source::inputs::CompilationInputs;
#[cfg(feature = "native")]
use kagari_mir::program::VerifiedMirProgram;
#[cfg(feature = "source")]
use std::sync::Arc;

/// No PreparedProgram Rc/cache, executable lease, native backend or callback capture.
#[derive(Debug, Clone, Default)]
pub(crate) struct RetainedPreparation {
    #[cfg(feature = "source")]
    pub(crate) compilation: Option<Arc<CompilationInputs>>,
    #[cfg(feature = "native")]
    pub(crate) mir: Option<VerifiedMirProgram>,
}
