//! Host-facing SDK with independent source and native compilation features.
//!
//! Disable default features for artifact-only loading and interpreter execution.
//! `source` adds analysis and artifact emission; `native` adds frontend-free MIR
//! preparation and trusted backend integration. Defaults enable both. Concrete
//! native backends are supplied by the host and are not production dependencies.
mod context;
mod engine;
mod error;
pub mod program;
mod runtime;

pub use context::{ExecutionContext, JitPolicy, PanicPolicy};
#[cfg(feature = "source")]
pub use engine::source::{ArtifactOptions, CheckedModule, CompileOptions, NativeInputExport};
pub use engine::{EngineConfig, KagariEngine};
pub use error::{
    CompilationPhase, DiagnosticLabel, EmbeddingDiagnostic, EmbeddingError, ReloadValidationError,
    RuntimeFailureKind,
};
#[cfg(feature = "source")]
pub use kagari_hir::typeck::ConstLimits;
pub use kagari_runtime::HostExposurePolicy;
#[cfg(feature = "source")]
pub use kagari_syntax::parser::ParseLimits;
pub use runtime::{KagariRuntime, LoadOptions, ReloadOptions};

#[cfg(feature = "source")]
pub type CompileResult<T> = Result<T, EmbeddingError>;
pub type LoadResult<T> = Result<T, EmbeddingError>;
pub type RunResult<T> = Result<T, EmbeddingError>;
pub type ReloadResult<T> = Result<T, EmbeddingError>;
use kagari_bytecode::KbcArtifact;
pub type BytecodeArtifact = KbcArtifact;
