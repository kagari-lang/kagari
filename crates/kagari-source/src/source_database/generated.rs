use super::{CapturedSource, SourceDatabase, SourceSnapshot, validate_module};
use crate::identity::FileId;
use kagari_common::cancellation::CancellationToken;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Only keys carry identity across refreshes. Display aliases address the current bytes.
#[derive(Debug, Default)]
pub(super) struct GeneratedDocuments {
    pub(super) keys: BTreeMap<String, FileId>,
    displays: BTreeMap<String, FileId>,
}

impl GeneratedDocuments {
    pub(super) fn lookup(&self, name: &str) -> Option<FileId> {
        self.keys
            .get(name)
            .or_else(|| self.displays.get(name))
            .copied()
    }
}

impl SourceDatabase {
    /// Finds a live read-only document by its logical key or current display name.
    pub fn generated_file_id(&self, name: &str) -> Option<FileId> {
        self.generated.lookup(&self.source_name(name).ok()?)
    }

    /// Replaces only the generated documents, retaining the editable layers and package environment.
    /// A stable key retains its file ID; changed text or diagnostic origin gets a new revision.
    /// Invalid, conflicting, or cancelled batches publish neither files nor lookup aliases.
    pub fn replace_generated(
        &mut self,
        sources: Vec<CapturedSource>,
        cancel: &CancellationToken,
    ) -> Result<SourceSnapshot, String> {
        cancel.check().map_err(|_| "source capture cancelled")?;
        let revision = self.next_revision();
        let mut generated = GeneratedDocuments::default();
        let mut files = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut modules = BTreeSet::new();
        for (key, document) in &self.documents {
            cancel.check().map_err(|_| "source capture cancelled")?;
            names.insert(key.clone());
            // Whole captures can have diagnostic labels that aren't addressable source names.
            if let Ok(display) = self.source_name(&document.display_name) {
                names.insert(display);
            }
            modules.insert(document.module.clone());
            if let Some(file) = self.snapshot.file(document.id) {
                files.insert(document.id, file.clone());
            }
        }
        let mut changed = sources.len() != self.generated.keys.len();
        for source in sources {
            cancel.check().map_err(|_| "source capture cancelled")?;
            let key = self.source_name(&source.key)?;
            let display = self.source_name(&source.display_name)?;
            validate_module(&source.module)?;
            if !modules.insert(source.module.clone()) {
                return Err(format!(
                    "module `{}` is already bound to another source",
                    source.module
                ));
            }
            if !names.insert(key.clone()) {
                return Err(format!(
                    "generated source key `{key}` conflicts with another source"
                ));
            }
            if display != key && !names.insert(display.clone()) {
                return Err(format!(
                    "generated display name `{display}` conflicts with another source"
                ));
            }
            let id = match self.generated.keys.get(&key) {
                Some(&id) if self.snapshot.files[&id].module_identity() != &source.module => {
                    return Err(format!(
                        "generated source key `{key}` belongs to a different module"
                    ));
                }
                Some(&id) => id,
                None => FileId::fresh(),
            };
            let file = self.captured_file(id, revision, &source);
            changed |= file.revision() == revision;
            files.insert(id, file);
            if display != key {
                generated.displays.insert(display, id);
            }
            generated.keys.insert(key, id);
        }
        cancel.check().map_err(|_| "source capture cancelled")?;
        if changed {
            self.generated = generated;
            self.snapshot.files = Arc::new(files);
            self.snapshot.revision = revision;
        }
        Ok(self.snapshot())
    }
}
