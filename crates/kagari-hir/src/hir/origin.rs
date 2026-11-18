//! Source and structured registration attribution of lowered records.

use kagari_common::{identity::table::DefinitionId, span::Span};

/// Attribution within the matching lowering and source or definition scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOrigin {
    /// Byte range in the lowering's source, including synthetic recovery anchors.
    Source(Span),
    /// Registered declaration responsible for this node, without authored bytes.
    Structured { declaration: DefinitionId },
}

impl NodeOrigin {
    /// Returns the range carried by source lowering, if available.
    pub fn source_span(self) -> Option<Span> {
        match self {
            Self::Source(span) => Some(span),
            Self::Structured { .. } => None,
        }
    }

    /// Returns the byte anchor for diagnostics on the matching input.
    ///
    /// Structured records use an empty anchor internally. Use [Self::source_span]
    /// for source navigation and [Self::Structured] for registration attribution.
    pub fn diagnostic_span(self) -> Span {
        self.source_span().unwrap_or_default()
    }
}
