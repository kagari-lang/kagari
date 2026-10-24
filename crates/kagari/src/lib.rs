//! Host-facing SDK with independent source and native compilation features.
//!
//! Disable default features for artifact-only loading and interpreter execution.
//! `source` adds analysis and artifact emission; `native` adds frontend-free MIR
//! preparation and trusted backend integration. Defaults enable both. Concrete
//! native backends are supplied by the host and are not production dependencies.
//! `web` supplies browser/Worker timing on wasm32-unknown-unknown independently
//! of `source`. Browser products do not support dynamic-library AOT loading.
#[cfg(all(target_family = "wasm", feature = "aot-load"))]
compile_error!("Kagari AOT dynamic-library loading/building is unsupported on WASM targets");

#[cfg(all(feature = "aot-load", not(target_family = "wasm")))]
pub mod aot;
pub mod context;
pub mod engine;
pub mod error;
pub mod program;
pub mod runtime;
#[cfg(feature = "source")]
pub mod update;

use crate::error::EmbeddingError;

#[cfg(feature = "source")]
pub type CompileResult<T> = Result<T, EmbeddingError>;
pub type LoadResult<T> = Result<T, EmbeddingError>;
pub type RunResult<T> = Result<T, EmbeddingError>;
pub type ReloadResult<T> = Result<T, EmbeddingError>;
use kagari_bytecode::artifact::KbcArtifact;
pub type BytecodeArtifact = KbcArtifact;

#[cfg(test)]
mod test_support;
