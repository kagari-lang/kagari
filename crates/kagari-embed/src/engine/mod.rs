//! Engine configuration and optional source compilation state.
pub mod builder;
#[cfg(feature = "source")]
pub mod source;
use crate::{
    context::ExecutionContext, engine::builder::KagariEngineBuilder, runtime::KagariRuntime,
};

#[cfg(feature = "source")]
use kagari_hir::analysis::AnalysisDatabase;
#[cfg(feature = "source")]
use kagari_hir::native::render::{DeclarationSource, declaration_source};
#[cfg(feature = "source")]
use kagari_runtime::native::foundation;
use kagari_runtime::{Runtime, RuntimeConfig, error::RuntimeError, native::module::NativeModule};
#[cfg(feature = "source")]
use kagari_source::source_database::SourceDatabase;
#[cfg(feature = "source")]
use std::{cell::RefCell, sync::Arc};

#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub default_runtime: RuntimeConfig,
}

#[derive(Debug)]
pub struct KagariEngine {
    config: EngineConfig,
    native_modules: Vec<NativeModule>,
    #[cfg(feature = "source")]
    foundation: Vec<NativeModule>,
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

    /// Install application modules in addition to the mandatory foundation.
    pub fn with_native_modules(
        config: EngineConfig,
        native_modules: Vec<NativeModule>,
    ) -> Result<Self, RuntimeError> {
        let mut validation = Runtime::new(config.default_runtime.clone());
        #[cfg(feature = "source")]
        let foundation = foundation::modules()?;
        for module in &native_modules {
            module.install(&mut validation)?;
        }
        #[cfg(feature = "source")]
        let analysis = {
            let mut analysis = AnalysisDatabase::default();
            let modules = foundation
                .iter()
                .chain(&native_modules)
                .map(|module| module.to_declaration().map(Arc::new))
                .collect::<Result<Vec<_>, _>>()?;
            analysis.set_native_modules(modules);
            RefCell::new(analysis)
        };
        Ok(Self {
            config,
            native_modules,
            #[cfg(feature = "source")]
            foundation,
            #[cfg(feature = "source")]
            sources: RefCell::default(),
            #[cfg(feature = "source")]
            analysis,
        })
    }

    #[cfg(feature = "source")]
    pub fn native_declaration_sources(&self) -> Vec<DeclarationSource> {
        self.foundation
            .iter()
            .chain(&self.native_modules)
            .map(|module| {
                declaration_source(&module.to_declaration().expect("checked module identities"))
                    .expect("checked module presentation")
            })
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
