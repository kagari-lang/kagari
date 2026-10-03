use crate::{
    identity::{FileId, FileSpan, Revision},
    line_index::{LineIndex, Position, PositionEncoding},
};
use kagari_common::{identity::ModuleIdentity, span::Span};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct CopiedSource {
    local: Span,
    original: Span,
    source: Arc<SourceFile>,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    id: FileId,
    origin: FileId,
    inline_range: Option<Span>,
    revision: Revision,
    module: ModuleIdentity,
    name: String,
    text: String,
    lines: LineIndex,
    copies: Vec<CopiedSource>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let name = name.into();
        let id = FileId::fresh();
        Self {
            id,
            origin: id,
            inline_range: None,
            revision: Revision::default(),
            module: ModuleIdentity::single_file(name.clone()),
            name,
            lines: LineIndex::new(&text),
            text,
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
        &self.text
    }

    pub fn id(&self) -> FileId {
        self.id
    }

    /// The physical source containing this file's text ranges. Generated inline
    /// module sources preserve byte offsets and line endings from their origin.
    pub fn origin_id(&self) -> FileId {
        self.origin
    }

    pub fn inline_module(
        parent: &Self,
        name: &str,
        text: String,
        id: FileId,
        inline_range: Span,
    ) -> Self {
        let mut module = parent.module.clone();
        module.path.push(name.to_owned());
        Self {
            id,
            origin: parent.origin,
            inline_range: Some(inline_range),
            revision: parent.revision,
            module,
            name: parent.name.clone(),
            lines: LineIndex::new(&text),
            text,
            copies: vec![],
        }
    }

    pub fn inline_range(&self) -> Option<Span> {
        self.inline_range
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn span(&self, range: Span) -> Option<FileSpan> {
        self.text.get(range.start..range.end)?;
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
            file: self.origin,
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
    ) -> Result<(), &'static str> {
        let text = self
            .text
            .get(local.start..local.end)
            .ok_or("invalid copy range")?;
        if source.text.get(original.start..original.end) != Some(text)
            || self
                .copies
                .iter()
                .any(|copy| local.start < copy.local.end && copy.local.start < local.end)
            || !source.copies.is_empty()
        {
            return Err("source copy must be exact, disjoint and directly authored");
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
        if location.file == self.origin && location.revision == self.revision {
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
        self.lines.position(&self.text, offset, encoding)
    }

    pub fn offset(&self, position: Position, encoding: PositionEncoding) -> Option<usize> {
        self.lines.offset(&self.text, position, encoding)
    }

    pub(crate) fn with_identity(
        mut self,
        id: FileId,
        revision: Revision,
        module: ModuleIdentity,
    ) -> Self {
        self.id = id;
        self.origin = id;
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
        let mut view = SourceFile::new("native.kgr", "type Native;\ntrait Add {}\nfn next();");
        let local = Span::new(13, 25);
        let authored = Span::new(6, 18);
        view.add_copy(local, original.clone(), authored).unwrap();
        let location = view.span(Span::new(19, 22)).unwrap();
        assert_eq!(location, original.span(Span::new(12, 15)).unwrap());
        assert_eq!(view.local_range(location), Some(Span::new(19, 22)));
        assert_eq!(view.copied_sources().next().unwrap().id(), original.id());
        assert!(view.add_copy(local, original.clone(), authored).is_err());
        assert!(
            view.add_copy(Span::new(0, 4), original.clone(), Span::new(12, 15))
                .is_err()
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
