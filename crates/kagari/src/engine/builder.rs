//! A mutable registration transaction sealed by `build`.
use crate::{
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
};
#[cfg(feature = "source")]
use kagari_compiler::source::core_library;
#[cfg(feature = "source")]
use kagari_hir::{
    analysis::core_source::packaged_modules, imports::environment::ImportEnvironment,
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
    pub(super) imports: ImportEnvironment,
    #[cfg(feature = "source")]
    pub(super) source_root: Option<String>,
}

impl KagariEngineBuilder {
    pub(super) fn empty() -> NativeResult<Self> {
        Ok(Self {
            config: EngineConfig::default(),
            modules: Vec::new(),
            catalog: authoring_catalog(&[])?,
            installation: NativeInstallation::prepare(&[])?,
            #[cfg(feature = "source")]
            declaration_cache: None,
            #[cfg(feature = "source")]
            imports: ImportEnvironment::default(),
            #[cfg(feature = "source")]
            source_root: None,
        })
    }

    pub fn config(&mut self, config: EngineConfig) -> &mut Self {
        self.config = config;
        self
    }

    /// Author APIs against installed modules and, with the source frontend, verified core declarations.
    /// A core declaration is not a Native registration.
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
        let catalog = authoring_catalog(&staged)?;
        self.modules = staged;
        self.catalog = catalog;
        self.installation = installation;
        Ok(())
    }

    /// Set the sealed import selections for registration checking and publication.
    /// Construction checks the complete scopes of bundled core and registrations
    /// that do not need project owners. Project importers and registrations that
    /// depend on them are checked with the project snapshot. Fresh sessions inherit
    /// all selections; [`KagariEngine::set_import_environment`] replaces them for
    /// one session.
    #[cfg(feature = "source")]
    pub fn import_environment(&mut self, imports: ImportEnvironment) -> &mut Self {
        self.imports = imports;
        self
    }

    /// Publish immutable tooling documents, never compiler inputs.
    ///
    /// Building publishes the source-independent registrations. Use
    /// [`KagariEngine::native_declaration_sources`] with the authored project to
    /// include registrations that depend on its declarations. Without a cache,
    /// only an explicit tooling request renders text, with no filesystem IO.
    /// Publication cannot give a structured program a complete source manifest.
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
        let sources = SourceDatabase::new(root)?;
        self.source_root = Some(sources.root().to_owned());
        Ok(self)
    }

    pub fn build(self) -> Result<KagariEngine, EmbeddingError> {
        KagariEngine::from_builder(self)
    }
}

fn authoring_catalog(modules: &[NativeModule]) -> NativeResult<DeclarationCatalog> {
    let catalog = DeclarationCatalog::from_modules(&modules.iter().collect::<Vec<_>>())?;
    #[cfg(feature = "source")]
    let catalog = catalog.with_verified_modules(
        core_library::verified().map_err(RuntimeError::module_validation)?,
        packaged_modules()
            .iter()
            .map(|module| module.module_identity()),
    )?;
    Ok(catalog)
}
