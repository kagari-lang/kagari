//! Source presentation and filesystem publication belong to the embedding SDK.
use crate::{engine::builder::KagariEngineBuilder, error::EmbeddingError};
use blake3::hash as content_hash;
use kagari_common::cancellation::CancellationToken;
use kagari_hir::{
    analysis::AnalysisDatabase,
    native::render::{DeclarationSource, declaration_source},
};
use kagari_source::source_database::{SourceDatabase, normalize_source_name};
use std::{
    fs::{self, OpenOptions},
    io::{Error, ErrorKind, Write},
    path::Path,
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
        .map(|module| module.to_declaration().map(Arc::new))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EmbeddingError::Registration { error })?;
    let mut sources = providers
        .iter()
        .map(|module| declaration_source(module, &providers))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EmbeddingError::Source {
            message: error.to_string(),
        })?;
    let mut analysis = AnalysisDatabase::default();
    analysis
        .set_native_sources(providers.clone(), sources.clone())
        .map_err(|error| EmbeddingError::Source {
            message: format!("{error:?}"),
        })?;
    // Validate all generated modules before publishing navigation locations.
    let signatures = analysis
        .signatures(
            SourceDatabase::default().snapshot(),
            &CancellationToken::default(),
        )
        .map_err(|error| EmbeddingError::Source {
            message: format!("{error:?}"),
        })?;
    for file in signatures.declaration_snapshot().files() {
        let checked = signatures
            .file(file.source().id())
            .expect("checked native signatures");
        if !checked.diagnostics().is_empty() {
            return Err(EmbeddingError::Source {
                message: format!(
                    "invalid registered declarations in {}: {:?}",
                    file.source().name(),
                    checked.diagnostics()
                ),
            });
        }
    }
    if let Some(root) = &builder.declaration_cache {
        for source in &mut sources {
            source.uri = materialize(root, &source.text)?;
        }
        analysis
            .set_native_sources(providers, sources.clone())
            .map_err(|error| EmbeddingError::Source {
                message: format!("{error:?}"),
            })?;
    }
    Ok((analysis, sources))
}

fn cache_error(path: &Path, error: Error) -> EmbeddingError {
    EmbeddingError::DeclarationCache {
        path: path.to_owned(),
        error,
    }
}

/// Identical text shares one immutable file. A doc-only change gets a new target.
fn materialize(root: &Path, text: &str) -> Result<String, EmbeddingError> {
    let directory = root.join(RENDERER_VERSION);
    fs::create_dir_all(&directory).map_err(|error| cache_error(&directory, error))?;
    let directory = fs::canonicalize(&directory).map_err(|error| cache_error(&directory, error))?;
    let hash = content_hash(text.as_bytes()).to_hex();
    let path = directory.join(format!("{hash}.kgr"));
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
