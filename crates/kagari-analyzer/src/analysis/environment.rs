//! Analysis-thread bootstrap from authoritative declarations, without filesystem IO.
#[cfg(test)]
use kagari_common::identity::table::DefinitionTableId;
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_hir::{
    analysis::{
        AnalysisDatabase, editor::EditorAnalysis, error::AnalysisError,
        signatures::SignatureSnapshot,
    },
    native::NativeSource,
};
use kagari_source::{
    publication::{PublicationSource, SourceDocument, SourceStore, module_path},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_stdlib::{catalog, sources as standard_sources};
use kagari_types::declaration::module::ModuleDecl;
use std::sync::Arc;

#[cfg(any(test, feature = "verification"))]
use kagari_hir::native::render::declaration_source;

const MAX_PUBLICATION_BYTES: usize = 16 * 1024 * 1024;

/// Kept entirely on the analysis worker. Only publication bytes cross to the IO
/// owner; publication failure restricts navigation without discarding semantics.
pub(crate) struct PreparedEnvironment {
    /// Retains the validated native-only context for this provider generation.
    /// User contexts are always created through context_database, never cloned.
    _database: AnalysisDatabase,
    pub publications: Arc<[SourceDocument]>,
    /// Source-owned allocations charged by the analysis owner's shared input budget.
    /// Publication strings are separate and bounded by MAX_PUBLICATION_BYTES.
    pub sources: Vec<Arc<SourceFile>>,
    source_root: String,
    providers: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    views: Vec<NativeSource>,
}

impl PreparedEnvironment {
    #[cfg(test)]
    pub(super) fn baseline_identity(&mut self) -> (DefinitionTableId, usize) {
        let snapshot = self
            ._database
            .snapshot(SourceSnapshot::default(), &CancellationToken::default())
            .expect("validated native-only baseline");
        (snapshot.definitions().id(), snapshot.definitions().len())
    }

    pub(super) fn signatures(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<SignatureSnapshot, AnalysisError> {
        self._database.signatures(SourceSnapshot::default(), cancel)
    }

    /// Queries an exact installed presentation view through its validated native-only context.
    pub(super) fn editor(
        &mut self,
        name: &str,
        offset: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<EditorAnalysis>, AnalysisError> {
        let signatures = self
            ._database
            .signatures(SourceSnapshot::default(), cancel)?;
        let file = signatures
            .declaration_snapshot()
            .files()
            .find(|file| file.source().name() == name)
            .map(|file| file.source().id());
        match file {
            Some(file) => self
                ._database
                .editor(SourceSnapshot::default(), file, offset, cancel),
            None => Ok(None),
        }
    }

    /// Installs validated immutable inputs into a fresh definition context.
    /// Native lowering is prepared by that context's first query. Cloning the
    /// baseline database would share its append-only identity writer and retain
    /// names from retired user contexts for the environment's entire lifetime.
    pub fn context_database(&self) -> Result<AnalysisDatabase, String> {
        let mut database = AnalysisDatabase::new(&self.source_root)?;
        database
            .set_native_sources(self.providers.clone(), self.views.clone())
            .map_err(|error| format!("{error:?}"))?;
        Ok(database)
    }
}

/// Obtains the standard semantic catalog without native preparation or execution.
pub(crate) fn prepare_standard(
    source_root: &str,
    documents: &SourceStore,
    cancel: &CancellationToken,
) -> Result<PreparedEnvironment, String> {
    check(cancel)?;
    let providers = catalog::shared().map_err(|error| error.to_string())?;
    if providers.len() != standard_sources::DECLARATIONS.len() {
        return Err("bundled standard source/provider inventory differs".into());
    }
    let sources = providers
        .iter()
        .map(|provider| {
            let source =
                standard_sources::declaration(&provider.records().identity).ok_or_else(|| {
                    format!(
                        "missing bundled standard source for {}",
                        provider.records().identity
                    )
                })?;
            Ok(NativeSource {
                uri: format!("kagari-stdlib:///{}", source.path),
                text: source.text.into(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    prepare_sources(source_root, documents, providers, sources, cancel)
}

/// Renders and validates one explicit provider generation at its final source paths.
/// This is the embedded/test injection boundary; providers must form a closed
/// declaration environment. Source-dependent application providers are not guessed.
#[cfg(any(test, feature = "verification"))]
pub(crate) fn prepare(
    source_root: &str,
    documents: &SourceStore,
    providers: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    cancel: &CancellationToken,
) -> Result<PreparedEnvironment, String> {
    let sources = providers
        .iter()
        .map(|provider| {
            declaration_source(provider, &providers)
                .map(Into::into)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    prepare_sources(source_root, documents, providers, sources, cancel)
}

fn prepare_sources(
    source_root: &str,
    documents: &SourceStore,
    providers: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    mut sources: Vec<NativeSource>,
    cancel: &CancellationToken,
) -> Result<PreparedEnvironment, String> {
    check(cancel)?;
    let mut publication_sources = Vec::with_capacity(providers.len());
    let mut publication_bytes = 0usize;
    for (provider, source) in providers.iter().zip(&sources) {
        check(cancel)?;
        publication_bytes = publication_bytes
            .checked_add(source.text.len())
            .filter(|bytes| *bytes <= MAX_PUBLICATION_BYTES)
            .ok_or_else(|| "native declarations exceed 16 MiB publication limit".to_owned())?;
        let path = source
            .uri
            .strip_prefix("kagari-stdlib:///")
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let identity = &provider.records().identity;
                module_path(&identity.package.0, &identity.path)
            });
        publication_sources.push(PublicationSource {
            path,
            text: Arc::from(source.text.as_str()),
        });
    }
    let package = if sources
        .iter()
        .all(|source| source.uri.starts_with("kagari-stdlib:///"))
    {
        "stdlib"
    } else {
        "hosts"
    };
    let publications = documents
        .plan(package, publication_sources)
        .map_err(|error| error.to_string())?;
    for (source, publication) in sources.iter_mut().zip(&publications) {
        source.uri = publication.source_name().into();
    }
    let mut database = AnalysisDatabase::new(source_root)?;
    database
        .set_native_sources(providers.clone(), sources.clone())
        .map_err(|error| format!("{error:?}"))?;
    // Installation checks only inventory length. Ordinary native analysis owns
    // source/registration correspondence, signatures and default-body validation.
    let snapshot = database
        .snapshot(SourceSnapshot::default(), cancel)
        .map_err(|error| format!("{error:?}"))?;
    let mut retained_sources = Vec::new();
    for file in snapshot.declaration_snapshot().files() {
        check(cancel)?;
        let checked = snapshot
            .file(file.source().id())
            .ok_or_else(|| "native analysis omitted a registered source".to_owned())?;
        if !checked.result().diagnostics().is_empty() {
            return Err(format!(
                "invalid registered declarations in {}: {:?}",
                file.source().name(),
                checked.result().diagnostics()
            ));
        }
        retained_sources.push(Arc::new(file.source().clone()));
        retained_sources.extend(file.source().copied_sources().cloned().map(Arc::new));
    }
    check(cancel)?;
    Ok(PreparedEnvironment {
        _database: database,
        publications: publications.into(),
        sources: retained_sources,
        source_root: source_root.into(),
        providers,
        views: sources,
    })
}

fn check(cancel: &CancellationToken) -> Result<(), String> {
    cancel
        .check()
        .map_err(|_| "environment preparation cancelled".into())
}

#[cfg(test)]
mod tests;
