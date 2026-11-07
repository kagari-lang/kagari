//! Open-document protocol metadata over source-owned identities, text and snapshots.
use crate::{protocol::source_name, server::options::SessionLimits};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Uri,
};
use std::{collections::HashMap, sync::Arc};

#[cfg(any(test, feature = "verification"))]
pub(crate) mod verification;

/// One captured open incarnation. Unavailable input shadows disk without inventing text.
#[derive(Clone, Debug)]
pub(crate) struct DocumentInput {
    pub(crate) uri: Uri,
    pub(crate) source_name: String,
    pub(crate) version: i32,
    pub(crate) incarnation: u64,
    pub(crate) source: Option<Arc<SourceFile>>,
    pub(crate) unavailable: bool,
}

struct OpenDocument {
    uri: Uri,
    language_id: String,
    version: i32,
    incarnation: u64,
    bytes: usize,
    available: bool,
}

pub(crate) struct Documents {
    source: SourceDatabase,
    open: HashMap<String, OpenDocument>,
    limits: SessionLimits,
    bytes: usize,
    incarnation: u64,
    metadata_bytes: usize,
    synchronization_failed: bool,
}

impl Documents {
    pub(crate) fn new(source: SourceDatabase, limits: SessionLimits) -> Self {
        Self {
            source,
            open: HashMap::new(),
            limits,
            bytes: 0,
            incarnation: 0,
            metadata_bytes: 0,
            synchronization_failed: false,
        }
    }

    pub(crate) fn open(&mut self, params: DidOpenTextDocumentParams) -> Result<(), String> {
        let item = params.text_document;
        if item.uri.as_str().len() > self.limits.document_uri_bytes {
            self.synchronization_failed = true;
            return Err("document URI exceeds metadata limit; session synchronization lost".into());
        }
        let name = source_name(&item.uri)?;
        if let Some(document) = self.open.get(&name) {
            return Err(format!(
                "document already open as {} (language {})",
                document.uri.as_str(),
                document.language_id
            ));
        }
        if item.language_id != "kagari" {
            return Err(format!(
                "unsupported document language: {}",
                item.language_id
            ));
        }
        let metadata_bytes = name.len() + item.uri.as_str().len();
        if self.open.len() >= self.limits.open_documents
            || metadata_bytes
                > self
                    .limits
                    .document_metadata_bytes
                    .saturating_sub(self.metadata_bytes)
        {
            self.synchronization_failed = true;
            return Err(
                "open documents exceed metadata limits; session synchronization lost".into(),
            );
        }
        let Some(incarnation) = self.incarnation.checked_add(1) else {
            self.synchronization_failed = true;
            return Err("document incarnation exhausted".into());
        };
        let length = item.text.len();
        let available = length <= self.limits.open_file_bytes
            && length <= self.limits.open_total_bytes.saturating_sub(self.bytes);
        if available {
            self.source.set(&name, item.text, SourceLayer::Overlay)?;
        }
        let bytes = if available { length } else { 0 };
        self.bytes += bytes;
        self.metadata_bytes += metadata_bytes;
        self.incarnation = incarnation;
        self.open.insert(
            name,
            OpenDocument {
                uri: item.uri,
                language_id: item.language_id,
                version: item.version,
                incarnation,
                bytes,
                available,
            },
        );
        if available {
            Ok(())
        } else {
            Err("document synchronization unavailable: source byte limit exceeded".into())
        }
    }

    pub(crate) fn change(&mut self, params: DidChangeTextDocumentParams) -> Result<(), String> {
        let name = source_name(&params.text_document.uri)?;
        let document = self.open.get_mut(&name).ok_or("document is not open")?;
        if params.text_document.version <= document.version {
            return Err(format!(
                "document version {} is not newer than {}",
                params.text_document.version, document.version
            ));
        }
        if params
            .content_changes
            .iter()
            .any(|change| change.range.is_some() || change.range_length.is_some())
        {
            return Err("full synchronization does not accept ranged changes".into());
        }
        // Validate the whole notification first. With full replacements only the last
        // text survives; no intermediate source state needs publication.
        if let Some(change) = params.content_changes.into_iter().last() {
            let remaining = self.bytes - document.bytes;
            let length = change.text.len();
            if length > self.limits.open_file_bytes
                || length > self.limits.open_total_bytes.saturating_sub(remaining)
            {
                self.source.close_overlay(&name)?;
                self.bytes = remaining;
                document.bytes = 0;
                document.available = false;
                document.version = params.text_document.version;
                return Err(
                    "document synchronization unavailable: source byte limit exceeded".into(),
                );
            }
            self.source.set(&name, change.text, SourceLayer::Overlay)?;
            self.bytes = remaining + length;
            document.bytes = length;
            document.available = true;
        }
        document.version = params.text_document.version;
        Ok(())
    }

    pub(crate) fn close(&mut self, params: DidCloseTextDocumentParams) -> Result<(), String> {
        let name = source_name(&params.text_document.uri)?;
        if !self.open.contains_key(&name) {
            return Err("document is not open".into());
        }
        self.source.remove(&name)?;
        if let Some(document) = self.open.remove(&name) {
            self.bytes -= document.bytes;
            self.metadata_bytes -= name.len() + document.uri.as_str().len();
        }
        Ok(())
    }

    /// Metadata or identity exhaustion loses an open buffer's authority.
    /// The session must terminate instead of silently falling back to disk.
    pub(crate) fn synchronization_failed(&self) -> bool {
        self.synchronization_failed
    }

    /// Retains exact source snapshots and protocol stamps without IO or text copying.
    pub(crate) fn inputs(&self) -> Vec<DocumentInput> {
        let snapshot = self.source.snapshot();
        let mut inputs = self
            .open
            .iter()
            .map(|(name, document)| {
                let source = document.available.then(|| {
                    self.source
                        .file_id(name)
                        .and_then(|id| snapshot.file(id).cloned())
                        .expect("available open document has an overlay source")
                });
                DocumentInput {
                    uri: document.uri.clone(),
                    source_name: name.clone(),
                    version: document.version,
                    incarnation: document.incarnation,
                    source,
                    unavailable: !document.available,
                }
            })
            .collect::<Vec<_>>();
        inputs.sort_by(|left, right| left.uri.as_str().cmp(right.uri.as_str()));
        inputs
    }
}

#[cfg(test)]
mod tests;
