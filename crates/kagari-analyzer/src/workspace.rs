//! Live editor sources, buffer lifetimes and their matching analysis inputs.

use kagari_common::cancellation::CancellationToken;
use kagari_hir::{
    analysis::{AnalysisDatabase, AnalysisSnapshot, error::AnalysisError},
    imports::environment::ImportEnvironment,
};
use kagari_source::{
    identity::FileId,
    source_database::{
        CapturedSource, SourceDatabase, SourceDatabaseError, SourceLayer, SourceSnapshot,
    },
};
use std::{collections::HashMap, fs, io::ErrorKind, path::Path};

#[cfg(test)]
mod tests;

/// Owns the editable project and the import selections for its current Host export.
/// Analysis uses bundled core and ordinary source only; there is no registration input.
pub struct EditorWorkspace {
    sources: SourceDatabase,
    analysis: AnalysisDatabase,
    // Key by the name that was opened: a generated display path can retire while still open.
    buffers: HashMap<String, BufferKind>,
}

#[derive(Clone, Copy)]
enum BufferKind {
    Editable,
    Generated,
}

impl EditorWorkspace {
    /// Takes ownership of the project's base documents, module bindings and package environment.
    pub fn new(sources: SourceDatabase) -> Result<Self, SourceDatabaseError> {
        let analysis = AnalysisDatabase::new(sources.root())?;
        Ok(Self {
            sources,
            analysis,
            buffers: HashMap::new(),
        })
    }

    /// Read-only access to source identities and immutable text snapshots.
    pub fn sources(&self) -> &SourceDatabase {
        &self.sources
    }

    /// Opens a user overlay, capturing the local disk base if the project has not supplied one.
    /// A generated key or display path may only open its exact published text, without an overlay.
    pub fn open(&mut self, name: &str, text: String) -> Result<FileId, String> {
        let name = self
            .sources
            .source_name(name)
            .map_err(|error| error.to_string())?;
        if self.buffers.contains_key(&name) {
            return Err(format!("document already open: {name}"));
        }
        let (id, kind) = if let Some(id) = self.sources.generated_file_id(&name) {
            if self
                .sources
                .snapshot()
                .file(id)
                .expect("live generated file")
                .text()
                != text
            {
                return Err("open text differs from read-only generated source".into());
            }
            (id, BufferKind::Generated)
        } else {
            self.capture_missing_base(&name)?;
            (
                self.sources
                    .set(&name, text, SourceLayer::Overlay)
                    .map_err(|error| error.to_string())?,
                BufferKind::Editable,
            )
        };
        self.buffers.insert(name, kind);
        Ok(id)
    }

    fn capture_missing_base(&mut self, name: &str) -> Result<(), String> {
        let path = Path::new(name);
        if !path.is_absolute()
            || self
                .sources
                .file_id(name)
                .is_some_and(|id| self.sources.snapshot().file(id).is_some())
        {
            return Ok(());
        }
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => self
                .sources
                .load_file(name)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Ok(_) => Err(format!("{name}: not a regular source file")),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("{name}: {error}")),
        }
    }

    /// Replaces the full text of an open, editable document. An open generated view
    /// remains read-only even after its name disappears from the live export.
    pub fn change(&mut self, name: &str, text: String) -> Result<(), String> {
        let name = self
            .sources
            .source_name(name)
            .map_err(|error| error.to_string())?;
        match self.buffers.get(&name) {
            Some(BufferKind::Editable) => self
                .sources
                .set(&name, text, SourceLayer::Overlay)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Some(BufferKind::Generated) => Err("generated source is read-only".into()),
            None => Err("document is not open".into()),
        }
    }

    /// Closes a view. User buffers reveal their base (or disappear); generated views
    /// own no overlay, so closing one never changes the current export.
    pub fn close(&mut self, name: &str) -> Result<(), String> {
        let name = self
            .sources
            .source_name(name)
            .map_err(|error| error.to_string())?;
        if let BufferKind::Editable = self.buffers.get(&name).ok_or("document is not open")? {
            self.sources
                .close_overlay(&name)
                .map_err(|error| error.to_string())?;
        }
        self.buffers.remove(&name);
        Ok(())
    }

    /// Installs one complete export: stable keys, display paths, module identities and bytes,
    /// together with the exact import selections supplied for those documents and the project.
    /// Invalid or cancelled source replacement leaves both the sources and selections unchanged.
    pub fn replace_generated(
        &mut self,
        documents: Vec<CapturedSource>,
        imports: ImportEnvironment,
        cancel: &CancellationToken,
    ) -> Result<SourceSnapshot, SourceDatabaseError> {
        let snapshot = self.sources.replace_generated(documents, cancel)?;
        self.analysis.set_import_environment(imports);
        Ok(snapshot)
    }

    /// Checks the effective sources against the current export's import environment.
    /// The returned result retains both across subsequent edits, closes and Host refreshes.
    pub fn analyze(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<AnalysisSnapshot, AnalysisError> {
        self.analysis.snapshot(self.sources.snapshot(), cancel)
    }
}
