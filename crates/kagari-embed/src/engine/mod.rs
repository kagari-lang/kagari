//! Engine configuration and optional source compilation state.
#[cfg(feature = "source")]
pub(crate) mod source;

use crate::context::ExecutionContext;
use crate::runtime::KagariRuntime;
#[cfg(feature = "source")]
use kagari_common::source_database::SourceDatabase;
#[cfg(feature = "source")]
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{Runtime, RuntimeConfig};
#[cfg(feature = "source")]
use std::cell::RefCell;

#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub default_runtime: RuntimeConfig,
}

#[derive(Debug, Default)]
pub struct KagariEngine {
    config: EngineConfig,
    #[cfg(feature = "source")]
    sources: RefCell<SourceDatabase>,
    #[cfg(feature = "source")]
    analysis: RefCell<AnalysisDatabase>,
}

impl KagariEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            #[cfg(feature = "source")]
            sources: RefCell::default(),
            #[cfg(feature = "source")]
            analysis: RefCell::default(),
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn runtime(&self, context: ExecutionContext) -> KagariRuntime {
        let mut config = self.config.default_runtime.clone();
        config.security = context.security_context();
        config.host_exposure = context.host_policy.clone();
        config.resources = context.resources;
        KagariRuntime::new(Runtime::new(config), context)
    }
}
