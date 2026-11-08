//! Native declaration validation and publication orchestration for the embedding SDK.
use crate::{engine::builder::KagariEngineBuilder, error::EmbeddingError};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_hir::{
    analysis::AnalysisDatabase,
    native::{NativeSource, render::declaration_source},
};
use kagari_source::{
    publication::{PublicationError, PublicationSource, SourceStore, module_path},
    source_database::SourceSnapshot,
};
use kagari_stdlib::sources as standard_sources;
use kagari_types::declaration::module::ModuleDecl;
use std::{collections::BTreeSet, sync::Arc};

pub(super) fn prepare(
    builder: &KagariEngineBuilder,
    source_root: &str,
) -> Result<(AnalysisDatabase, Vec<NativeSource>), EmbeddingError> {
    let providers = builder
        .modules
        .iter()
        .map(|module| Arc::new(module.to_declaration()))
        .collect::<Vec<_>>();
    let mut sources = providers
        .iter()
        .map(|module| {
            if let Some(source) = standard_sources::declaration(&module.records().identity) {
                Ok(NativeSource {
                    uri: format!("kagari-stdlib:///{}", source.path),
                    text: source.text.into(),
                })
            } else {
                declaration_source(module, &providers).map(Into::into)
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EmbeddingError::Source {
            message: error.to_string(),
        })?;
    // Analyze the final source identity once; publish files only after checking.
    let mut publications = Vec::new();
    if let Some(root) = &builder.declaration_cache {
        let documents = SourceStore::open(root).map_err(cache_error)?;
        for package in ["stdlib", "hosts"] {
            let indices = sources
                .iter()
                .enumerate()
                .filter_map(|(index, source)| {
                    let standard = source.uri.starts_with("kagari-stdlib:///");
                    (standard == (package == "stdlib")).then_some(index)
                })
                .collect::<Vec<_>>();
            let files = indices
                .iter()
                .map(|index| {
                    let source = &sources[*index];
                    let identity = &providers[*index].records().identity;
                    let path = source
                        .uri
                        .strip_prefix("kagari-stdlib:///")
                        .map(str::to_owned)
                        .unwrap_or_else(|| module_path(&identity.package.0, &identity.path));
                    PublicationSource {
                        path,
                        text: Arc::from(source.text.as_str()),
                    }
                })
                .collect();
            let planned = documents.plan(package, files).map_err(cache_error)?;
            for (index, document) in indices.into_iter().zip(planned) {
                sources[index].uri = document.source_name().into();
                publications.push(document);
            }
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

fn cache_error(error: PublicationError) -> EmbeddingError {
    EmbeddingError::DeclarationCache {
        path: error.path,
        error: error.error,
    }
}
