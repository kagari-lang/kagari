//! Engine configuration and optional source compilation state.
pub mod builder;
#[cfg(feature = "source")]
pub mod source;
use crate::{
    context::ExecutionContext, engine::builder::KagariEngineBuilder, runtime::KagariRuntime,
};

use kagari_abi::declaration::render::DeclarationSource;
#[cfg(feature = "source")]
use kagari_common::source_database::SourceDatabase;
#[cfg(feature = "source")]
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{Runtime, RuntimeConfig, error::RuntimeError, native::module::NativeModule};
#[cfg(feature = "source")]
use std::cell::RefCell;

#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub default_runtime: RuntimeConfig,
}

#[derive(Debug)]
pub struct KagariEngine {
    config: EngineConfig,
    native_modules: Vec<NativeModule>,
    #[cfg(feature = "source")]
    sources: RefCell<SourceDatabase>,
    #[cfg(feature = "source")]
    analysis: RefCell<AnalysisDatabase>,
}

impl KagariEngine {
    pub fn builder() -> KagariEngineBuilder {
        KagariEngineBuilder::default()
    }

    pub fn new(config: EngineConfig) -> Self {
        Self::builder()
            .config(config)
            .build()
            .expect("default native module installation")
    }

    /// Install exactly this set of optional modules. Built-in and application
    /// packages use the same checked installation path; use builder() for defaults.
    pub fn with_native_modules(
        config: EngineConfig,
        native_modules: Vec<NativeModule>,
    ) -> Result<Self, RuntimeError> {
        let mut validation = Runtime::new(config.default_runtime.clone());
        for module in &native_modules {
            module.install(&mut validation)?;
        }
        #[cfg(feature = "source")]
        let analysis = {
            let mut analysis = AnalysisDatabase::default();
            let modules = native_modules
                .iter()
                .map(|module| module.declaration().clone())
                .collect();
            analysis.set_native_modules(modules);
            RefCell::new(analysis)
        };
        Ok(Self {
            config,
            native_modules,
            #[cfg(feature = "source")]
            sources: RefCell::default(),
            #[cfg(feature = "source")]
            analysis,
        })
    }

    pub fn native_declaration_sources(&self) -> Vec<DeclarationSource> {
        self.native_modules
            .iter()
            .map(NativeModule::declaration_source)
            .collect()
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn runtime(&self, context: ExecutionContext) -> KagariRuntime {
        let config = self.config.default_runtime.clone();
        let mut runtime = Runtime::new(config);
        for module in &self.native_modules {
            module
                .install(&mut runtime)
                .expect("engine validated native module installation");
        }
        KagariRuntime::new(runtime, context)
    }
}

impl Default for KagariEngine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}
