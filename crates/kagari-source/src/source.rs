use crate::{
    identity::{FileId, FileSpan, Revision},
    line_index::{LineIndex, Position, PositionEncoding},
};
use kagari_common::{identity::ModuleIdentity, span::Span};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone)]
struct CopiedSource {
    local: Span,
    original: Span,
    source: Arc<SourceFile>,
}

#[derive(Debug)]
struct SourceContent {
    text: String,
    lines: LineIndex,
}

/// The logical part of the physical text represented by a [`SourceFile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceExtent {
    /// The entire source; its logical and physical file IDs are the same.
    Whole,
    /// An inline module body in the physical source's original byte coordinates.
    Inline {
        /// File containing the physical text, even for nested inline modules.
        origin: FileId,
        /// Module content range, excluding its enclosing braces.
        range: Span,
    },
}

/// A copied fragment could not be recorded as source provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceCopyError {
    #[error("invalid copy range")]
    InvalidLocalRange,
    #[error("source copy must be exact, disjoint and directly authored")]
    OriginalTextMismatch,
    #[error("source copy must be exact, disjoint and directly authored")]
    OverlappingLocalRange,
    #[error("source copy must be exact, disjoint and directly authored")]
    NotDirectlyAuthored,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    id: FileId,
    extent: SourceExtent,
    revision: Revision,
    module: ModuleIdentity,
    name: String,
    content: Arc<SourceContent>,
    copies: Vec<CopiedSource>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let name = name.into();
        let id = FileId::fresh();
        Self {
            id,
            extent: SourceExtent::Whole,
            revision: Revision::default(),
            module: ModuleIdentity::single_file(name.clone()),
            name,
            content: Arc::new(SourceContent {
                lines: LineIndex::new(&text),
                text,
            }),
            copies: vec![],
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn module_identity(&self) -> &ModuleIdentity {
        &self.module
    }

    pub fn text(&self) -> &str {
        &self.content.text
    }

    pub fn id(&self) -> FileId {
        self.id
    }

    /// The whole source or the inline module body selected within its physical text.
    pub fn extent(&self) -> SourceExtent {
        self.extent
    }

    /// The physical source containing this file's text ranges. Generated inline
    /// module sources preserve byte offsets and line endings from their origin.
    pub fn origin_id(&self) -> FileId {
        match self.extent {
            SourceExtent::Whole => self.id,
            SourceExtent::Inline { origin, .. } => origin,
        }
    }

    /// Creates a logical child sharing immutable physical text and line indexing.
    /// Byte ranges retain physical coordinates; no source text is rewritten or parsed.
    /// A supplied range is checked against the syntax when preparing analysis input.
    pub fn inline_module(parent: &Self, name: &str, id: FileId, range: Span) -> Self {
        let mut module = parent.module.clone();
        module.path.push(name.to_owned());
        Self {
            id,
            extent: SourceExtent::Inline {
                origin: parent.origin_id(),
                range,
            },
            revision: parent.revision,
            module,
            name: parent.name.clone(),
            content: parent.content.clone(),
            copies: parent.copies.clone(),
        }
    }

    /// The body range for an inline module, in physical byte coordinates.
    pub fn inline_range(&self) -> Option<Span> {
        match self.extent {
            SourceExtent::Whole => None,
            SourceExtent::Inline { range, .. } => Some(range),
        }
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn span(&self, range: Span) -> Option<FileSpan> {
        self.content.text.get(range.start..range.end)?;
        if let Some(copy) = self
            .copies
            .iter()
            .find(|copy| copy.local.start <= range.start && range.end <= copy.local.end)
        {
            return copy.source.span(Span::new(
                copy.original.start + range.start - copy.local.start,
                copy.original.start + range.end - copy.local.start,
            ));
        }
        Some(FileSpan {
            file: self.origin_id(),
            revision: self.revision,
            range,
        })
    }

    /// Record an exact copied fragment, retaining its authoritative source.
    /// Disjoint, byte-identical ranges keep generated analysis and navigation
    /// coordinates separate without accepting a stale or rewritten origin.
    pub fn add_copy(
        &mut self,
        local: Span,
        source: Arc<SourceFile>,
        original: Span,
    ) -> Result<(), SourceCopyError> {
        let text = self
            .content
            .text
            .get(local.start..local.end)
            .ok_or(SourceCopyError::InvalidLocalRange)?;
        if source.content.text.get(original.start..original.end) != Some(text) {
            return Err(SourceCopyError::OriginalTextMismatch);
        }
        if self
            .copies
            .iter()
            .any(|copy| local.start < copy.local.end && copy.local.start < local.end)
        {
            return Err(SourceCopyError::OverlappingLocalRange);
        }
        if source.extent != SourceExtent::Whole || !source.copies.is_empty() {
            return Err(SourceCopyError::NotDirectlyAuthored);
        }
        self.copies.push(CopiedSource {
            local,
            original,
            source,
        });
        Ok(())
    }

    pub fn copied_sources(&self) -> impl Iterator<Item = &SourceFile> {
        self.copies.iter().map(|copy| copy.source.as_ref())
    }

    /// Translate a navigation location back into this analysis view.
    pub fn local_range(&self, location: FileSpan) -> Option<Span> {
        if location.file == self.origin_id() && location.revision == self.revision {
            return (self.span(location.range)? == location).then_some(location.range);
        }
        self.copies.iter().find_map(|copy| {
            (location.file == copy.source.id
                && location.revision == copy.source.revision
                && copy.original.start <= location.range.start
                && location.range.end <= copy.original.end)
                .then(|| {
                    Span::new(
                        copy.local.start + location.range.start - copy.original.start,
                        copy.local.start + location.range.end - copy.original.start,
                    )
                })
        })
    }

    pub fn position(&self, offset: usize, encoding: PositionEncoding) -> Option<Position> {
        self.content
            .lines
            .position(&self.content.text, offset, encoding)
    }

    pub fn offset(&self, position: Position, encoding: PositionEncoding) -> Option<usize> {
        self.content
            .lines
            .offset(&self.content.text, position, encoding)
    }

    /// Assigns the database identity of a newly captured whole source.
    pub(crate) fn with_identity(
        mut self,
        id: FileId,
        revision: Revision,
        module: ModuleIdentity,
    ) -> Self {
        assert_eq!(self.extent, SourceExtent::Whole);
        self.id = id;
        self.revision = revision;
        self.module = module;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_copies_preserve_authored_navigation_and_analysis_offsets() {
        let original = Arc::new(SourceFile::new("core.kgr", "// π\ntrait Add {}"));
        let retained = original.as_ref().clone();
        assert!(Arc::ptr_eq(&retained.content, &original.content));
        let mut view = SourceFile::new("native.kgr", "type Native;\ntrait Add {}\nfn next();");
        let local = Span::new(13, 25);
        let authored = Span::new(6, 18);
        view.add_copy(local, original.clone(), authored).unwrap();
        let location = view.span(Span::new(19, 22)).unwrap();
        assert_eq!(location, original.span(Span::new(12, 15)).unwrap());
        assert_eq!(view.local_range(location), Some(Span::new(19, 22)));
        assert_eq!(view.copied_sources().next().unwrap().id(), original.id());
        assert_eq!(
            view.add_copy(local, original.clone(), authored),
            Err(SourceCopyError::OverlappingLocalRange)
        );
        assert_eq!(
            view.add_copy(Span::new(0, 4), original.clone(), Span::new(12, 15)),
            Err(SourceCopyError::OriginalTextMismatch)
        );
        assert_eq!(
            view.add_copy(
                Span::new(0, view.text().len() + 1),
                original.clone(),
                authored
            ),
            Err(SourceCopyError::InvalidLocalRange)
        );
        let inline = Arc::new(SourceFile::inline_module(
            &original,
            "nested",
            FileId::fresh(),
            authored,
        ));
        assert!(Arc::ptr_eq(&inline.content, &original.content));
        assert_eq!(inline.span(authored), original.span(authored));
        for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            let position = inline.position(authored.start, encoding).unwrap();
            assert_eq!(Some(position), original.position(authored.start, encoding));
            assert_eq!(inline.offset(position, encoding), Some(authored.start));
        }
        let mut other_view = SourceFile::new("other.kgr", view.text());
        assert_eq!(
            other_view.add_copy(local, inline, authored),
            Err(SourceCopyError::NotDirectlyAuthored)
        );
        let mut stale = location;
        stale.revision = Revision(1);
        assert!(view.local_range(stale).is_none());
        assert!(
            view.span(Span::new(0, 4))
                .is_some_and(|span| span.file == view.id())
        );
    }
}
