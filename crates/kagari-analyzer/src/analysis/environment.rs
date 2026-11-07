//! Analysis-thread bootstrap from authoritative declarations, without filesystem IO.
#[cfg(test)]
use kagari_common::identity::table::DefinitionTableId;
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_hir::{
    analysis::AnalysisDatabase,
    native::render::{DeclarationSource, declaration_source},
};
use kagari_source::{
    generated::{GeneratedDocument, GeneratedDocuments},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_stdlib::catalog;
use kagari_types::declaration::module::ModuleDecl;
use std::sync::Arc;

const MAX_PUBLICATION_BYTES: usize = 16 * 1024 * 1024;

/// Kept entirely on the analysis worker. Only publication bytes cross to the IO
/// owner; the matching environment epoch becomes usable after publication succeeds.
pub(crate) struct PreparedEnvironment {
    /// Retains the validated native-only context for this provider generation.
    /// User contexts are always created through context_database, never cloned.
    _database: AnalysisDatabase,
    pub publications: Arc<[GeneratedDocument]>,
    /// Source-owned allocations charged by the analysis owner's shared input budget.
    /// Publication strings are separate and bounded by MAX_PUBLICATION_BYTES.
    pub sources: Vec<Arc<SourceFile>>,
    source_root: String,
    providers: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    views: Vec<DeclarationSource>,
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
    documents: &GeneratedDocuments,
    cancel: &CancellationToken,
) -> Result<PreparedEnvironment, String> {
    check(cancel)?;
    let providers = catalog::shared().map_err(|error| error.to_string())?;
    prepare(source_root, documents, providers, cancel)
}

/// Renders and validates one explicit provider generation at its final source paths.
/// This is the embedded/test injection boundary; providers must form a closed
/// declaration environment. Source-dependent application providers are not guessed.
pub(crate) fn prepare(
    source_root: &str,
    documents: &GeneratedDocuments,
    providers: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    cancel: &CancellationToken,
) -> Result<PreparedEnvironment, String> {
    check(cancel)?;
    let mut sources = Vec::with_capacity(providers.len());
    let mut publications = Vec::with_capacity(providers.len());
    let mut publication_bytes = 0usize;
    for provider in &providers {
        check(cancel)?;
        let mut source =
            declaration_source(provider, &providers).map_err(|error| error.to_string())?;
        publication_bytes = publication_bytes
            .checked_add(source.text.len())
            .filter(|bytes| *bytes <= MAX_PUBLICATION_BYTES)
            .ok_or_else(|| "generated declarations exceed 16 MiB publication limit".to_owned())?;
        let publication = documents.plan(Arc::from(source.text.as_str()));
        source.uri = publication.source_name().into();
        sources.push(source);
        publications.push(publication);
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
