//! Native declaration validation and publication orchestration for the embedding SDK.
use crate::{engine::builder::KagariEngineBuilder, error::EmbeddingError};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_hir::{
    analysis::AnalysisDatabase,
    native::render::{DeclarationSource, NATIVE_RENDERER_NAMESPACE, declaration_source},
};
use kagari_source::{
    generated::{GeneratedDocumentError, GeneratedDocuments},
    source_database::SourceSnapshot,
};
use kagari_types::declaration::module::ModuleDecl;
use std::{collections::BTreeSet, sync::Arc};

pub(super) fn prepare(
    builder: &KagariEngineBuilder,
    source_root: &str,
) -> Result<(AnalysisDatabase, Vec<DeclarationSource>), EmbeddingError> {
    let providers = builder
        .modules
        .iter()
        .map(|module| Arc::new(module.to_declaration()))
        .collect::<Vec<_>>();
    let mut sources = providers
        .iter()
        .map(|module| declaration_source(module, &providers))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EmbeddingError::Source {
            message: error.to_string(),
        })?;
    // Analyze the final source identity once; publish files only after checking.
    let mut publications = Vec::new();
    if let Some(root) = &builder.declaration_cache {
        let documents =
            GeneratedDocuments::open(root, NATIVE_RENDERER_NAMESPACE).map_err(cache_error)?;
        for source in &mut sources {
            let document = documents.plan(Arc::from(source.text.as_str()));
            source.uri = document.source_name().into();
            publications.push(document);
        }
    }
    let mut analysis =
        AnalysisDatabase::new(source_root).map_err(|message| EmbeddingError::Source { message })?;
    analysis
        .set_native_sources(providers.clone(), sources.clone())
        .map_err(|error| EmbeddingError::Source {
            message: format!("{error:?}"),
        })?;
    // Source-dependent declarations retain checked expected contracts, but their
    // actual trait definitions arrive with the user's source snapshot. Validate
    // the closed source-independent subset now; ordinary compilation validates
    // the complete set after those source providers are available.
    let mut validation = source_independent_modules(&providers)
        .map(|indices| {
            let mut validation = analysis.clone();
            validation
                .set_native_sources(
                    indices
                        .iter()
                        .map(|index| providers[*index].clone())
                        .collect(),
                    indices
                        .iter()
                        .map(|index| sources[*index].clone())
                        .collect(),
                )
                .map_err(|error| EmbeddingError::Source {
                    message: format!("{error:?}"),
                })?;
            Ok::<_, EmbeddingError>(validation)
        })
        .transpose()?;
    let snapshot = validation
        .as_mut()
        .unwrap_or(&mut analysis)
        .snapshot(SourceSnapshot::default(), &CancellationToken::default())
        .map_err(|error| EmbeddingError::Source {
            message: format!("{error:?}"),
        })?;
    for file in snapshot.declaration_snapshot().files() {
        let checked = snapshot
            .file(file.source().id())
            .expect("checked native bodies");
        if !checked.result().diagnostics().is_empty() {
            return Err(EmbeddingError::Source {
                message: format!(
                    "invalid registered declarations in {}: {:?}",
                    file.source().name(),
                    checked.result().diagnostics()
                ),
            });
        }
    }
    for document in publications {
        document.publish().map_err(cache_error)?;
    }
    Ok((analysis, sources))
}

/// Preserve the fully warmed baseline when every provider is source independent.
/// Otherwise select the closed subset whose source can be checked before user input.
fn source_independent_modules(
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
) -> Option<Vec<usize>> {
    let mut deferred = providers
        .iter()
        .filter(|module| !module.records().trait_references.is_empty())
        .map(|module| module.records().identity.clone())
        .collect::<BTreeSet<_>>();
    if deferred.is_empty() {
        return None;
    }
    loop {
        let before = deferred.len();
        for module in providers {
            if module
                .records()
                .dependencies
                .iter()
                .any(|dependency| deferred.contains(dependency))
            {
                deferred.insert(module.records().identity.clone());
            }
        }
        if deferred.len() == before {
            break;
        }
    }
    Some(
        providers
            .iter()
            .enumerate()
            .filter(|(_, module)| !deferred.contains(&module.records().identity))
            .map(|(index, _)| index)
            .collect(),
    )
}

fn cache_error(error: GeneratedDocumentError) -> EmbeddingError {
    EmbeddingError::DeclarationCache {
        path: error.path,
        error: error.error,
    }
}
