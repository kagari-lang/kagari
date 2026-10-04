//! A mutable registration transaction sealed by `build`.
use crate::{
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    native::{binding::NativeResult, catalog::DeclarationCatalog, module::NativeModule},
};
use kagari_stdlib as stdlib;
use std::collections::HashSet;
#[cfg(feature = "source")]
use std::path::PathBuf;

#[derive(Debug)]
pub struct KagariEngineBuilder {
    pub(super) config: EngineConfig,
    pub(super) modules: Vec<NativeModule>,
    catalog: DeclarationCatalog,
    #[cfg(feature = "source")]
    pub(super) declaration_cache: Option<PathBuf>,
}

impl KagariEngineBuilder {
    pub(super) fn standard() -> NativeResult<Self> {
        let mut builder = Self {
            config: EngineConfig::default(),
            modules: Vec::new(),
            catalog: DeclarationCatalog::default(),
            #[cfg(feature = "source")]
            declaration_cache: None,
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
        let mut validation = Runtime::new(self.config.default_runtime.clone());
        NativeModule::install_all(&staged, &mut validation)?;
        let catalog = DeclarationCatalog::from_modules(&staged.iter().collect::<Vec<_>>())?;
        self.modules = staged;
        self.catalog = catalog;
        Ok(())
    }

    /// Materialize immutable declaration files for tooling. Omission keeps all
    /// declaration views in memory and performs no filesystem IO.
    #[cfg(feature = "source")]
    pub fn declaration_cache(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.declaration_cache = Some(path.into());
        self
    }

    pub fn build(self) -> Result<KagariEngine, EmbeddingError> {
        KagariEngine::from_builder(self)
    }
}
