//! Engine configuration and sealed registration state.
pub mod builder;
#[cfg(feature = "source")]
mod declarations;
#[cfg(feature = "source")]
pub mod source;
use crate::{
    context::ExecutionContext, engine::builder::KagariEngineBuilder, error::EmbeddingError,
    runtime::KagariRuntime,
};
#[cfg(feature = "source")]
use kagari_hir::{analysis::AnalysisDatabase, native::render::DeclarationSource};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    native::{binding::NativeResult, module::NativeModule},
};
#[cfg(feature = "source")]
use kagari_source::source_database::SourceDatabase;
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
    declaration_sources: Vec<DeclarationSource>,
    #[cfg(feature = "source")]
    sources: RefCell<SourceDatabase>,
    #[cfg(feature = "source")]
    analysis: RefCell<AnalysisDatabase>,
}

impl KagariEngine {
    pub fn builder() -> NativeResult<KagariEngineBuilder> {
        KagariEngineBuilder::standard()
    }

    pub fn new(config: EngineConfig) -> Self {
        let mut builder = Self::builder().expect("standard registration");
        builder.config(config);
        builder.build().expect("default native module installation")
    }

    pub fn with_native_modules(
        config: EngineConfig,
        modules: Vec<NativeModule>,
    ) -> Result<Self, EmbeddingError> {
        let mut builder =
            Self::builder().map_err(|error| EmbeddingError::Registration { error })?;
        builder.config(config);
        builder
            .install_all(modules)
            .map_err(|error| EmbeddingError::Registration { error })?;
        builder.build()
    }

    fn from_builder(builder: KagariEngineBuilder) -> Result<Self, EmbeddingError> {
        #[cfg(feature = "source")]
        let (analysis, declaration_sources) = declarations::prepare(&builder)?;
        Ok(Self {
            config: builder.config,
            native_modules: builder.modules,
            #[cfg(feature = "source")]
            declaration_sources,
            #[cfg(feature = "source")]
            sources: RefCell::default(),
            #[cfg(feature = "source")]
            analysis: RefCell::new(analysis),
        })
    }

    #[cfg(feature = "source")]
    pub fn native_declaration_sources(&self) -> &[DeclarationSource] {
        &self.declaration_sources
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn runtime(&self, context: ExecutionContext) -> KagariRuntime {
        let mut runtime = Runtime::new(self.config.default_runtime.clone());
        NativeModule::install_all(&self.native_modules, &mut runtime)
            .expect("engine validated native modules");
        KagariRuntime::new(runtime, context)
    }
}

impl Default for KagariEngine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}
