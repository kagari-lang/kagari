//! Engine configuration and sealed registration state.
pub mod builder;
#[cfg(feature = "source")]
mod declarations;
#[cfg(feature = "source")]
pub mod packages;
#[cfg(feature = "source")]
pub mod source;
use crate::{
    context::ExecutionContext, engine::builder::KagariEngineBuilder, error::EmbeddingError,
    runtime::Runtime,
};
#[cfg(feature = "source")]
use kagari_hir::{analysis::AnalysisDatabase, native::render::DeclarationSource};
use kagari_runtime::{
    RuntimeConfig,
    native::{binding::NativeResult, installation::NativeInstallation, module::NativeModule},
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
    installation: NativeInstallation,
    #[cfg(feature = "source")]
    native_analysis: AnalysisDatabase,
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
        let sources = match &builder.source_root {
            Some(root) => {
                SourceDatabase::new(root).map_err(|message| EmbeddingError::Source { message })?
            }
            None => SourceDatabase::default(),
        };
        #[cfg(feature = "source")]
        let (analysis, declaration_sources) = declarations::prepare(&builder, sources.root())?;
        Ok(Self {
            config: builder.config,
            installation: builder.installation,
            #[cfg(feature = "source")]
            native_analysis: analysis.clone(),
            #[cfg(feature = "source")]
            declaration_sources,
            #[cfg(feature = "source")]
            sources: RefCell::new(sources),
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

    /// Start an independent source session with the same sealed native providers.
    ///
    /// Reuses only the native-only baseline checked at engine construction. User
    /// sources and query-setting changes are not inherited. The captured source
    /// root is retained without consulting the current working directory.
    /// Native callbacks retain their original captures; create a new builder when
    /// those captures or provider declarations must differ.
    pub fn new_session(&self, config: EngineConfig) -> Self {
        Self {
            config,
            installation: self.installation.clone(),
            #[cfg(feature = "source")]
            native_analysis: self.native_analysis.clone(),
            #[cfg(feature = "source")]
            declaration_sources: self.declaration_sources.clone(),
            #[cfg(feature = "source")]
            sources: RefCell::new(
                SourceDatabase::new(self.sources.borrow().root()).expect("captured source root"),
            ),
            #[cfg(feature = "source")]
            analysis: RefCell::new(self.native_analysis.clone()),
        }
    }

    pub fn runtime(&self, context: ExecutionContext) -> Runtime {
        let runtime = self
            .installation
            .runtime(self.config.default_runtime.clone());
        Runtime::new(runtime, context)
    }
}

impl Default for KagariEngine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}

#[cfg(test)]
mod tests;
