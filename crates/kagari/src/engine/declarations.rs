//! Source presentation and filesystem publication belong to the embedding SDK.
use crate::{engine::builder::KagariEngineBuilder, error::EmbeddingError};
use blake3::hash as content_hash;
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_hir::{
    analysis::AnalysisDatabase,
    native::render::{DeclarationSource, declaration_source},
};
use kagari_source::source_database::{SourceDatabase, normalize_source_name};
use kagari_types::declaration::module::ModuleDecl;
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Error, ErrorKind, Write},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const RENDERER_VERSION: &str = "kgr-native-v1";
static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn prepare(
    builder: &KagariEngineBuilder,
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
    if let Some(root) = &builder.declaration_cache {
        for source in &mut sources {
            source.uri = source_name(&cache_path(root, &source.text)?)?;
        }
    }
    let mut analysis = AnalysisDatabase::default();
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
        .snapshot(
            SourceDatabase::default().snapshot(),
            &CancellationToken::default(),
        )
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
    if let Some(root) = &builder.declaration_cache {
        for source in &mut sources {
            source.uri = materialize(root, &source.text)?;
        }
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

fn cache_error(path: &Path, error: Error) -> EmbeddingError {
    EmbeddingError::DeclarationCache {
        path: path.to_owned(),
        error,
    }
}

/// Identical text shares one immutable file. A doc-only change gets a new target.
fn cache_path(root: &Path, text: &str) -> Result<PathBuf, EmbeddingError> {
    let directory = root.join(RENDERER_VERSION);
    fs::create_dir_all(&directory).map_err(|error| cache_error(&directory, error))?;
    let directory = fs::canonicalize(&directory).map_err(|error| cache_error(&directory, error))?;
    let hash = content_hash(text.as_bytes()).to_hex();
    Ok(directory.join(format!("{hash}.kgr")))
}

/// Publishes validated text atomically without replacing an existing snapshot's file.
fn materialize(root: &Path, text: &str) -> Result<String, EmbeddingError> {
    let path = cache_path(root, text)?;
    let directory = path.parent().expect("declaration cache directory");
    match fs::read(&path) {
        Ok(bytes) => {
            if bytes != text.as_bytes() {
                return Err(cache_error(
                    &path,
                    Error::new(
                        ErrorKind::InvalidData,
                        "declaration cache content differs from its identity",
                    ),
                ));
            }
            return source_name(&path);
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(cache_error(&path, error)),
    }
    let temporary = directory.join(format!(
        ".{}-{}.tmp",
        process::id(),
        TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let publish = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        // A hard link atomically publishes complete content without replacing an
        // existing target that a concurrent or older snapshot may already own.
        match fs::hard_link(&temporary, &path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if fs::read(&path)? == text.as_bytes() {
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorKind::InvalidData,
                        "declaration cache collision",
                    ))
                }
            }
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_file(&temporary);
    publish.map_err(|error| cache_error(&path, error))?;
    source_name(&path)
}

fn source_name(path: &Path) -> Result<String, EmbeddingError> {
    // SourceDatabase uses normalized absolute paths for physical files. Keep the
    // published view and the analyzed source name identical; LSP adapters can
    // encode this path as a file URI at their protocol boundary.
    let name = path.to_str().ok_or_else(|| {
        cache_error(
            path,
            Error::new(
                ErrorKind::InvalidInput,
                "declaration cache path is not UTF-8",
            ),
        )
    })?;
    // Windows canonicalization adds a verbatim disk prefix. Publish an ordinary
    // drive path before lexical source normalization replaces its separators.
    let name = name
        .strip_prefix(r"\\?\")
        .filter(|name| name.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(name);
    normalize_source_name(name)
        .map_err(|message| cache_error(path, Error::new(ErrorKind::InvalidInput, message)))
}
