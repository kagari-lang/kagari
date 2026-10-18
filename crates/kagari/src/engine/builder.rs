//! A mutable registration transaction sealed by `build`.
use crate::{
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult, catalog::DeclarationCatalog, installation::NativeInstallation,
        module::NativeModule,
    },
};
#[cfg(feature = "source")]
use kagari_source::source_database::SourceDatabase;
use kagari_stdlib as stdlib;
use std::collections::HashSet;
#[cfg(feature = "source")]
use std::path::PathBuf;

#[derive(Debug)]
pub struct KagariEngineBuilder {
    pub(super) config: EngineConfig,
    pub(super) modules: Vec<NativeModule>,
    catalog: DeclarationCatalog,
    pub(super) installation: NativeInstallation,
    #[cfg(feature = "source")]
    pub(super) declaration_cache: Option<PathBuf>,
    #[cfg(feature = "source")]
    pub(super) source_root: Option<String>,
}

impl KagariEngineBuilder {
    pub(super) fn standard() -> NativeResult<Self> {
        let mut builder = Self {
            config: EngineConfig::default(),
            modules: Vec::new(),
            catalog: DeclarationCatalog::default(),
            installation: NativeInstallation::prepare(&[])?,
            #[cfg(feature = "source")]
            declaration_cache: None,
            #[cfg(feature = "source")]
            source_root: None,
        };
        builder.install_all(stdlib::modules()?)?;
        Ok(builder)
    }

    pub fn config(&mut self, config: EngineConfig) -> &mut Self {
        self.config = config;
        self
    }

    /// Author application APIs against the complete installed declaration set.
    pub fn declarations(&self) -> &DeclarationCatalog {
        &self.catalog
    }

    pub fn install(&mut self, module: NativeModule) -> NativeResult<()> {
        self.install_all([module])
    }

    /// Validate a closed batch before publishing any module or declaration.
    pub fn install_all(
        &mut self,
        modules: impl IntoIterator<Item = NativeModule>,
    ) -> NativeResult<()> {
        let mut staged = self.modules.clone();
        staged.extend(modules);
        let mut identities = HashSet::new();
        for module in &staged {
            if !identities.insert(&module.declaration().identity) {
                return Err(RuntimeError::module_validation(format!(
                    "duplicate native module identity: {}",
                    module.declaration().identity
                )));
            }
        }
        let installation = NativeInstallation::prepare(&staged)?;
        let catalog = DeclarationCatalog::from_modules(&staged.iter().collect::<Vec<_>>())?;
        self.modules = staged;
        self.catalog = catalog;
        self.installation = installation;
        Ok(())
    }

    /// Materialize immutable declaration files for tooling. Omission keeps all
    /// declaration views in memory and performs no filesystem IO.
    #[cfg(feature = "source")]
    pub fn declaration_cache(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.declaration_cache = Some(path.into());
        self
    }

    /// Set the absolute root for source names without consulting the filesystem.
    ///
    /// Omission captures the native working directory at build time. Fresh sessions
    /// retain that root. Virtual URIs keep their own identity regardless of the root.
    /// Invalid or relative roots return a source error without changing the builder.
    #[cfg(feature = "source")]
    pub fn source_root(&mut self, root: &str) -> Result<&mut Self, EmbeddingError> {
        let sources =
            SourceDatabase::new(root).map_err(|message| EmbeddingError::Source { message })?;
        self.source_root = Some(sources.root().to_owned());
        Ok(self)
    }

    pub fn build(self) -> Result<KagariEngine, EmbeddingError> {
        KagariEngine::from_builder(self)
    }
}
