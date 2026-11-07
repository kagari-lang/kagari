//! Immutable open-buffer precedence over approved package filesystem discovery.
use crate::{
    documents::DocumentInput,
    protocol::source_name,
    workspace::{
        budget::{InputBudget, InputLease},
        origins::{input_error, native_key, physical_uri},
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_package::{
    error::PackageError,
    filesystem::FilesystemSources,
    input::InputTree,
    modules::{SourceInput, SourceText},
};
use kagari_source::source::{SourceContentId, SourceFile};
use lsp_types::Uri;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

/// One capture keeps the first acquired physical bytes stable across its contexts.
/// The cache is dropped after staging, and its identity set charges shared buffers.
pub(super) struct ReadCache {
    files: BTreeMap<String, Option<SourceText>>,
    overlays: BTreeMap<String, Overlay>,
    documents: BTreeMap<String, String>,
    contents: BTreeSet<SourceContentId>,
    leases: Vec<InputLease>,
    budget: InputBudget,
}

impl ReadCache {
    pub fn new(budget: InputBudget) -> Self {
        Self {
            files: BTreeMap::new(),
            overlays: BTreeMap::new(),
            documents: BTreeMap::new(),
            contents: BTreeSet::new(),
            leases: Vec::new(),
            budget,
        }
    }

    fn retain(&mut self, source: &SourceText) -> Result<(), PackageError> {
        if !self.contents.contains(&source.source.content_id()) {
            let lease = self
                .budget
                .retain([source.source.clone()])
                .map_err(|error| input_error(&source.origin, error.to_string()))?;
            self.contents.insert(source.source.content_id());
            self.leases.push(lease);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Overlay {
    source: Option<Arc<SourceFile>>,
    uri: Uri,
    conflict: bool,
}

pub(super) struct OverlayInput {
    filesystem: FilesystemSources,
    lexical_root: String,
    local_documents: BTreeSet<String>,
    cache: Rc<RefCell<ReadCache>>,
    errors: Vec<PackageError>,
}

impl OverlayInput {
    pub fn new(
        root: &Uri,
        documents: &[DocumentInput],
        cache: Rc<RefCell<ReadCache>>,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        cancel.check()?;
        let lexical_root =
            source_name(root).map_err(|message| input_error(root.as_str(), message))?;
        let filesystem = FilesystemSources::new(Path::new(&lexical_root))?;
        let mut result = Self {
            filesystem,
            lexical_root,
            local_documents: BTreeSet::new(),
            cache,
            errors: Vec::new(),
        };
        for document in documents {
            cancel.check()?;
            let Some(relative) = result.relative(&document.source_name) else {
                continue;
            };
            let path = match result.filesystem.approved_path(&relative) {
                Ok(path) => path,
                Err(error) => {
                    result.errors.push(error);
                    continue;
                }
            };
            let key = path
                .to_str()
                .ok_or_else(|| input_error(document.uri.as_str(), "source path is not UTF-8"))?
                .to_owned();
            result.local_documents.insert(document.source_name.clone());
            let mut cache = result.cache.borrow_mut();
            cache
                .documents
                .insert(document.source_name.clone(), key.clone());
            if let Some(previous) = cache.overlays.get_mut(&key) {
                if previous.source.as_ref().map(|source| source.text())
                    != document.source.as_ref().map(|source| source.text())
                {
                    previous.conflict = true;
                    previous.source = None;
                }
            } else {
                cache.overlays.insert(
                    key,
                    Overlay {
                        source: document.source.clone(),
                        uri: document.uri.clone(),
                        conflict: false,
                    },
                );
            }
        }
        Ok(result)
    }

    pub fn canonical_root(&self) -> Result<String, PackageError> {
        native_key(self.filesystem.root())
            .map_err(|message| input_error(&self.lexical_root, message))
    }

    pub fn relative(&self, name: &str) -> Option<String> {
        let relative = Path::new(name)
            .strip_prefix(Path::new(&self.lexical_root))
            .ok()?;
        Some(
            relative
                .components()
                .map(|part| part.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()?
                .join("/"),
        )
    }

    pub fn approved(&self, relative: &str) -> Result<PathBuf, PackageError> {
        self.filesystem
            .approved_path(if relative.is_empty() { "." } else { relative })
    }

    pub fn document_key(&self, name: &str) -> Option<String> {
        self.cache.borrow().documents.get(name).cloned()
    }

    pub fn admits_document(&self, name: &str) -> bool {
        self.local_documents.contains(name)
    }

    pub fn canonical_relative(&self, name: &str) -> Option<String> {
        if !self.admits_document(name) {
            return None;
        }
        let key = self.document_key(name)?;
        let relative = Path::new(&key).strip_prefix(self.filesystem.root()).ok()?;
        Some(
            relative
                .components()
                .map(|part| part.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()?
                .join("/"),
        )
    }

    pub fn uri(&self, key: &str) -> Option<Uri> {
        self.cache
            .borrow()
            .overlays
            .get(key)
            .map(|overlay| overlay.uri.clone())
            .or_else(|| physical_uri(key).ok())
    }

    pub fn errors(&self) -> &[PackageError] {
        &self.errors
    }

    pub fn conflicting(&self, key: &str) -> bool {
        self.cache
            .borrow()
            .overlays
            .get(key)
            .is_some_and(|overlay| overlay.conflict)
    }
}

impl SourceInput for OverlayInput {
    fn read(
        &self,
        relative: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<SourceText>, PackageError> {
        cancel.check()?;
        let canonical = self.approved(relative)?;
        let key = canonical
            .to_str()
            .ok_or_else(|| input_error(relative, "source path is not UTF-8"))?;
        // Manifest/lock inputs remain disk-owned even if a client labels them Kagari.
        let disk_configuration = relative.ends_with(".toml") || relative.ends_with("Kagari.lock");
        let overlay = self.cache.borrow().overlays.get(key).cloned();
        if !disk_configuration && let Some(overlay) = overlay {
            if overlay.conflict {
                return Err(input_error(
                    key,
                    "open URI aliases have conflicting source text",
                ));
            }
            let source = overlay
                .source
                .as_ref()
                .ok_or_else(|| input_error(key, "document synchronization is unavailable"))?;
            if source.text().len() > max_bytes {
                return Err(input_error(key, "source exceeds byte limit"));
            }
            let source = SourceText {
                key: key.into(),
                origin: key.into(),
                source: source.clone(),
            };
            self.cache.borrow_mut().retain(&source)?;
            return Ok(Some(source));
        }
        if let Some(cached) = self.cache.borrow().files.get(key).cloned() {
            if cached
                .as_ref()
                .is_some_and(|source| source.text().len() > max_bytes)
            {
                return Err(input_error(key, "source exceeds byte limit"));
            }
            return Ok(cached);
        }
        let source = self.filesystem.read(relative, max_bytes, cancel)?;
        let mut cache = self.cache.borrow_mut();
        if let Some(source) = &source {
            cache.retain(source)?;
        }
        cache.files.insert(key.into(), source.clone());
        Ok(source)
    }
}

impl InputTree for OverlayInput {
    fn directories(
        &self,
        directory: &str,
        max_entries: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PackageError> {
        let mut directories = self
            .filesystem
            .directories(directory, max_entries, cancel)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let approved = self.approved(directory)?;
        for key in self.cache.borrow().overlays.keys() {
            cancel.check()?;
            let Ok(relative) = Path::new(key).strip_prefix(&approved) else {
                continue;
            };
            let mut parts = relative.components();
            if let Some(Component::Normal(name)) = parts.next()
                && parts.next().is_some()
            {
                let name = name
                    .to_str()
                    .ok_or_else(|| input_error(key, "source path is not UTF-8"))?;
                directories.insert(name.to_owned());
                if directories.len() > max_entries {
                    return Err(input_error(directory, "directory discovery exceeds limit"));
                }
            }
        }
        Ok(directories.into_iter().collect())
    }
}
