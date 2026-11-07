//! Ordered acquisition inputs and retained, context-scoped source captures.
pub(crate) mod acquisition;
pub(crate) mod budget;
mod capture;
pub(crate) mod origins;
mod overlay;

#[cfg(test)]
mod tests;

use crate::documents::DocumentInput;
use crate::workspace::budget::{InputBudget, InputLease};
use kagari_common::identity::ModuleIdentity;
use kagari_source::{identity::FileId, source_database::SourceSnapshot};
use lsp_types::Uri;
use std::{collections::BTreeMap, error::Error, fmt, ops::Range, path::Path};

pub(crate) const MAX_CONTEXTS: usize = 128;
#[cfg(test)]
pub(crate) const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;

/// Exact open-buffer authority admitted by one ordered session generation.
#[derive(Clone, Debug)]
pub(crate) struct CaptureRequest {
    pub generation: u64,
    pub roots: Vec<Uri>,
    pub documents: Vec<DocumentInput>,
    _lease: InputLease,
}

impl CaptureRequest {
    pub fn new(
        generation: u64,
        roots: Vec<Uri>,
        documents: Vec<DocumentInput>,
        budget: &InputBudget,
    ) -> Result<Self, CaptureFailure> {
        let lease = budget.retain(
            documents
                .iter()
                .filter_map(|document| document.source.clone()),
        )?;
        Ok(Self {
            generation,
            roots,
            documents,
            _lease: lease,
        })
    }
}

/// Selected module identity is scoped by the physical workspace that supplied it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ContextKey {
    pub workspace: String,
    pub root: ModuleIdentity,
    pub epoch: u64,
}

/// Acquisition diagnostics retain a physical destination when one is known.
#[derive(Clone, Debug)]
pub(crate) struct CaptureIssue {
    pub uri: Option<Uri>,
    pub range: Option<Range<usize>>,
    pub message: String,
}

impl fmt::Display for CaptureIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(uri) = &self.uri {
            write!(formatter, "{}", uri.as_str())?;
            if let Some(range) = &self.range {
                write!(formatter, ":bytes {}..{}", range.start, range.end)?;
            }
            write!(formatter, ": {}", self.message)
        } else {
            self.message.fmt(formatter)
        }
    }
}

/// Package provenance paired with its editor-visible physical document.
#[derive(Clone, Debug)]
pub(crate) struct Origin {
    pub uri: Option<Uri>,
    pub module: ModuleIdentity,
    pub provider_key: String,
    pub read_only: bool,
}

impl Origin {
    /// Express the retained provider identity in the document-name namespace, without IO.
    pub fn normalized_key(&self) -> Result<String, String> {
        origins::native_key(Path::new(&self.provider_key))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ContextCapture {
    pub key: ContextKey,
    pub root: ModuleIdentity,
    pub sources: SourceSnapshot,
    pub origins: BTreeMap<FileId, Origin>,
    pub complete: bool,
    /// Every declared module was acquired; recovered body syntax may still be invalid.
    pub discovery_complete: bool,
    /// False for unresolved project graphs or conflicting physical overlays.
    /// Available text then supports document-only recovery, not semantic answers.
    pub semantic_available: bool,
    pub errors: Vec<CaptureIssue>,
    _lease: InputLease,
}

#[derive(Clone, Debug)]
pub(crate) struct DocumentContext {
    pub context: ContextKey,
    pub file: FileId,
    pub version: i32,
    pub incarnation: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceCapture {
    pub generation: u64,
    pub contexts: Vec<ContextCapture>,
    /// Keys use the session document owner's normalized source name.
    pub document_contexts: BTreeMap<String, DocumentContext>,
    /// Canonical physical identities captured before root reads, including buffers
    /// that cannot receive a semantic binding due to conflicting/unavailable text.
    /// Keys use normalized admitted document names; values use normalized provider keys.
    pub document_origins: BTreeMap<String, String>,
    pub errors: Vec<CaptureIssue>,
    _lease: InputLease,
}

impl WorkspaceCapture {
    /// Count selected contexts and admitted document buffers by shared content owner.
    pub fn retained_bytes(&self) -> usize {
        self._lease.retained_bytes()
    }
}

#[derive(Debug)]
pub(crate) enum CaptureFailure {
    Cancelled,
    OutOfOrder { previous: u64, requested: u64 },
    InvalidInput(String),
}

impl fmt::Display for CaptureFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("source acquisition cancelled"),
            Self::OutOfOrder {
                previous,
                requested,
            } => {
                write!(
                    formatter,
                    "capture generation {requested} does not follow {previous}"
                )
            }
            Self::InvalidInput(message) => message.fmt(formatter),
        }
    }
}

impl Error for CaptureFailure {}
