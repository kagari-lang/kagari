//! Host-facing SDK with independent source and native compilation features.
//!
//! Disable default features for artifact-only loading and interpreter execution.
//! `source` adds analysis and artifact emission; `native` adds frontend-free MIR
//! preparation and trusted backend integration. Defaults enable both. Concrete
//! native backends are supplied by the host and are not production dependencies.
pub mod context;
pub mod engine;
pub mod error;
pub mod program;
pub mod runtime;

use crate::error::EmbeddingError;

#[cfg(feature = "source")]
pub type CompileResult<T> = Result<T, EmbeddingError>;
pub type LoadResult<T> = Result<T, EmbeddingError>;
pub type RunResult<T> = Result<T, EmbeddingError>;
pub type ReloadResult<T> = Result<T, EmbeddingError>;
use kagari_bytecode::artifact::KbcArtifact;
pub type BytecodeArtifact = KbcArtifact;
