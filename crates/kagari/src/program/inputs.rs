//! Immutable preparation-side inputs retained by exact runtime publication token.

#[cfg(feature = "source")]
use crate::engine::source::inputs::CompilationInputs;
#[cfg(feature = "native")]
use kagari_mir::program::VerifiedMirProgram;
#[cfg(any(feature = "source", feature = "native"))]
use std::sync::Arc;

/// Shares immutable compiler inputs with prepared programs, never their Rc/cache,
/// executable leases, native backends or callback captures.
#[derive(Debug, Clone, Default)]
pub(crate) struct RetainedPreparation {
    #[cfg(feature = "source")]
    pub(crate) compilation: Option<Arc<CompilationInputs>>,
    #[cfg(feature = "native")]
    pub(crate) mir: Option<Arc<VerifiedMirProgram>>,
}
