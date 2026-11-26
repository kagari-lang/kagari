//! Engine configuration and sealed registration state.
pub mod builder;
#[cfg(feature = "source")]
pub mod declarations;
#[cfg(feature = "source")]
pub mod packages;
#[cfg(feature = "source")]
mod pins;
#[cfg(feature = "source")]
pub mod source;
#[cfg(feature = "source")]
use crate::engine::pins::InstallationPins;
use crate::{
    context::ExecutionContext, engine::builder::KagariEngineBuilder, error::EmbeddingError,
    runtime::Runtime,
};
#[cfg(feature = "source")]
use kagari_common::cancellation::CancellationToken;
#[cfg(feature = "source")]
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    RuntimeConfig,
    native::{binding::NativeResult, installation::NativeInstallation, module::NativeModule},
};
#[cfg(feature = "source")]
use kagari_source::source_database::{SourceDatabase, SourceSnapshot};
#[cfg(feature = "source")]
use std::{
    cell::RefCell,
    path::{self, PathBuf},
    sync::Arc,
};

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
    pins: Arc<InstallationPins>,
    #[cfg(feature = "source")]
    declaration_cache: Option<PathBuf>,
    #[cfg(feature = "source")]
    sources: RefCell<SourceDatabase>,
    #[cfg(feature = "source")]
    analysis: RefCell<AnalysisDatabase>,
}

impl KagariEngine {
    /// Construct an empty registration transaction; providers are installed explicitly.
    pub fn builder() -> NativeResult<KagariEngineBuilder> {
        KagariEngineBuilder::empty()
    }

    /// Construct an engine without Native providers or an implicit standard library.
    pub fn new(config: EngineConfig) -> Self {
        let mut builder = Self::builder().expect("empty registration");
        builder.config(config);
        builder.build().expect("empty native module installation")
    }

    /// Construct with exactly the supplied Native modules, checked as one batch.
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
        let declaration_cache = builder
            .declaration_cache
            .as_ref()
            .map(|root| {
                path::absolute(root).map_err(|error| EmbeddingError::DeclarationCache {
                    path: root.clone(),
                    error,
                })
            })
            .transpose()?;
        #[cfg(feature = "source")]
        let sources = match &builder.source_root {
            Some(root) => {
                SourceDatabase::new(root).map_err(|message| EmbeddingError::Source { message })?
            }
            None => SourceDatabase::default(),
        };
        #[cfg(feature = "source")]
        let analysis =
            declarations::prepare(&builder, sources.root(), declaration_cache.as_deref())?;
        Ok(Self {
            config: builder.config,
            installation: builder.installation,
            #[cfg(feature = "source")]
            native_analysis: analysis.clone(),
            #[cfg(feature = "source")]
            pins: Arc::new(InstallationPins::capture(&builder.modules)?),
            #[cfg(feature = "source")]
            declaration_cache,
            #[cfg(feature = "source")]
            sources: RefCell::new(sources),
            #[cfg(feature = "source")]
            analysis: RefCell::new(analysis),
        })
    }

    /// Render the installed Host API for an editor analyzing `project`.
    ///
    /// Supply authored project sources only, not an earlier export. The returned
    /// import selections apply to these exact module owners. Document keys survive
    /// edits to registered documentation; display paths point to immutable files
    /// if publication was requested, and otherwise are virtual. No files are read
    /// back as compiler inputs. Returns an error for unrepresentable declarations,
    /// invalid import selections, cancellation, or failed filesystem publication.
    #[cfg(feature = "source")]
    pub fn native_declaration_sources(
        &self,
        project: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<declarations::DeclarationSources, EmbeddingError> {
        let snapshot = self.declarations(project, cancel)?;
        declarations::export(&snapshot, self.declaration_cache.as_deref(), cancel)
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
            pins: self.pins.clone(),
            #[cfg(feature = "source")]
            declaration_cache: self.declaration_cache.clone(),
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
        let mut runtime = runtime;
        runtime.set_default_context(context);
        runtime
    }
}

impl Default for KagariEngine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}

#[cfg(test)]
mod tests;
