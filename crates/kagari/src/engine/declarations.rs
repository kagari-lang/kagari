//! Source presentation and filesystem publication belong to the embedding SDK.
use crate::{
    engine::{builder::KagariEngineBuilder, source::analysis_error},
    error::EmbeddingError,
};
use blake3::hash as content_hash;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, metadata::DefinitionMetadata},
};
use kagari_hir::{
    analysis::{
        AnalysisDatabase, core_source::packaged_modules, declarations::DeclarationSnapshot,
    },
    imports::environment::ImportEnvironment,
};
use kagari_source::{
    source::SourceFile,
    source_database::{CapturedSource, SourceSnapshot, normalize_source_name},
};
use kagari_syntax::parser::parse;
use kagari_types::declaration::module::ModuleDecl;
use std::{
    collections::BTreeSet,
    fmt::Write as _,
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

/// Generated Host documents and the effective import selections for one project capture.
///
/// Merge `documents` with the project's authored documents at the editor workspace
/// boundary. Analyze that snapshot with `imports`, bundled core and **no registrations**.
/// A `native host type` declaration identifies a handle but does not supply its
/// schema, an adapter, or permission to execute.
pub struct DeclarationSources {
    /// Exact generated text, keyed by logical owner independently of cache path or content.
    pub documents: Vec<CapturedSource>,
    /// Importer-relative package aliases, dependencies and preludes, including user importers.
    pub imports: ImportEnvironment,
}

pub(super) fn prepare(
    builder: &KagariEngineBuilder,
    source_root: &str,
    cache: Option<&Path>,
) -> Result<AnalysisDatabase, EmbeddingError> {
    let providers = builder
        .modules
        .iter()
        .map(|module| Arc::new(module.to_declaration()))
        .collect::<Vec<_>>();
    let mut analysis =
        AnalysisDatabase::new(source_root).map_err(|message| EmbeddingError::Source { message })?;
    let owners = construction_owners(&providers, &builder.imports);
    let mut imports = builder.imports.clone();
    imports.modules.retain(|owner, _| owners.contains(owner));
    let deferred_scopes = imports.modules.len() != builder.imports.modules.len();
    let closed: Vec<_> = providers
        .iter()
        .filter(|p| owners.contains(&p.records().identity))
        .cloned()
        .collect();
    let deferred_providers = closed.len() != providers.len();
    analysis.set_import_environment(imports);
    analysis.set_native_modules(closed);
    let snapshot = analysis
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
    if let Some(root) = cache {
        export(
            snapshot.declaration_snapshot(),
            Some(root),
            &CancellationToken::default(),
        )?;
    }
    // Compilation and explicit tooling requests must validate the complete sealed
    // inputs against their own project, not replay the construction-only subset.
    if deferred_providers {
        analysis.set_native_modules(providers);
    }
    if deferred_scopes {
        analysis.set_import_environment(builder.imports.clone());
    }
    Ok(analysis)
}

pub(super) fn export(
    snapshot: &DeclarationSnapshot,
    cache: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<DeclarationSources, EmbeddingError> {
    let mut documents = Vec::with_capacity(snapshot.native_modules().len());
    for provider in snapshot.native_modules() {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        let module = &provider.records().identity;
        let view = snapshot
            .render_native_document(provider, cancel)
            .map_err(analysis_error)?;
        // Check the actual exported bytes, not just the originating registration,
        // before publishing any content-addressed file from this request.
        let key = document_key(module);
        let source = SourceFile::new(&key, &view.text);
        let parsed =
            parse(&source, Default::default(), cancel).map_err(|_| EmbeddingError::Cancelled)?;
        if let Some(diagnostic) = parsed.diagnostics().first() {
            return Err(EmbeddingError::Source {
                message: format!("invalid generated declarations in {module}: {diagnostic:?}"),
            });
        }
        documents.push(CapturedSource {
            display_name: key.clone(),
            key,
            module: module.clone(),
            text: view.text,
        });
    }
    if let Some(root) = cache {
        for document in &mut documents {
            cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
            document.display_name = materialize(root, &document.key, &document.text)?;
        }
    }
    let sources = DeclarationSources {
        documents,
        imports: snapshot.import_environment().clone(),
    };
    cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
    Ok(sources)
}

/// A canonical package ID can contain `/` or `%`; it is one opaque URI component,
/// not part of the (identifier-only) module path. Content does not affect the key.
fn document_key(module: &ModuleIdentity) -> String {
    let mut key = String::from("kagari://native/");
    for byte in module.package.0.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            key.push(char::from(byte));
        } else {
            write!(key, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    key.push('/');
    key.push_str(&module.path.join("/"));
    key.push_str(".kgr");
    key
}

/// Only core and closed registrations can be checked before project acquisition.
/// Removing a provider can remove the last owner of an aliased package, not just
/// the target of a module edge. Defer the entire importer, never prune its scope.
fn construction_owners<'a>(
    providers: &'a [Arc<DefinitionMetadata<ModuleDecl>>],
    imports: &ImportEnvironment,
) -> BTreeSet<&'a ModuleIdentity> {
    let mut closed: Vec<_> = providers.iter().map(|p| p.records()).collect();
    loop {
        let owners: BTreeSet<_> = packaged_modules()
            .iter()
            .map(|core| core.module_identity())
            .chain(closed.iter().map(|module| &module.identity))
            .collect();
        let packages: BTreeSet<_> = owners.iter().map(|owner| &owner.package).collect();
        let previous = closed.len();
        closed.retain(|module| {
            module
                .dependencies
                .iter()
                .all(|owner| owners.contains(owner))
                && imports.modules.get(&module.identity).is_none_or(|scope| {
                    scope
                        .dependencies
                        .iter()
                        .chain(&scope.preludes)
                        .all(|owner| owners.contains(owner))
                        && scope
                            .packages
                            .dependencies
                            .values()
                            .all(|package| packages.contains(package))
                })
        });
        if previous == closed.len() {
            return owners;
        }
    }
}

fn cache_error(path: &Path, error: Error) -> EmbeddingError {
    EmbeddingError::DeclarationCache {
        path: path.to_owned(),
        error,
    }
}

/// Distinct document keys need distinct navigation targets even when their bytes match.
fn cache_path(root: &Path, key: &str, text: &str) -> Result<PathBuf, EmbeddingError> {
    let owner = content_hash(key.as_bytes()).to_hex();
    let directory = root.join(RENDERER_VERSION).join(owner.as_str());
    fs::create_dir_all(&directory).map_err(|error| cache_error(&directory, error))?;
    let directory = fs::canonicalize(&directory).map_err(|error| cache_error(&directory, error))?;
    let hash = content_hash(text.as_bytes()).to_hex();
    Ok(directory.join(format!("{hash}.kgr")))
}

/// Publishes validated text atomically without replacing an existing snapshot's file.
fn materialize(root: &Path, key: &str, text: &str) -> Result<String, EmbeddingError> {
    let path = cache_path(root, key, text)?;
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
