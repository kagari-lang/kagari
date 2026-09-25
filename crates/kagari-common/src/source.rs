#[derive(Debug, Clone)]
pub struct SourceFile {
    id: crate::identity::FileId,
    origin: crate::identity::FileId,
    inline_range: Option<crate::Span>,
    revision: crate::identity::Revision,
    module: crate::identity::ModuleIdentity,
    name: String,
    text: String,
    lines: crate::line_index::LineIndex,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let name = name.into();
        let id = crate::identity::FileId::fresh();
        Self {
            id,
            origin: id,
            inline_range: None,
            revision: crate::identity::Revision::default(),
            module: crate::identity::ModuleIdentity::single_file(name.clone()),
            name,
            lines: crate::line_index::LineIndex::new(&text),
            text,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn module_identity(&self) -> &crate::identity::ModuleIdentity {
        &self.module
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn id(&self) -> crate::identity::FileId {
        self.id
    }

    /// The physical source containing this file's text ranges. Generated inline
    /// module sources preserve byte offsets and line endings from their origin.
    pub fn origin_id(&self) -> crate::identity::FileId {
        self.origin
    }

    pub fn inline_module(
        parent: &Self,
        name: &str,
        text: String,
        id: crate::identity::FileId,
        inline_range: crate::Span,
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
            lines: crate::line_index::LineIndex::new(&text),
            text,
        }
    }

    pub fn inline_range(&self) -> Option<crate::Span> {
        self.inline_range
    }

    pub fn revision(&self) -> crate::identity::Revision {
        self.revision
    }

    pub fn span(&self, range: crate::Span) -> Option<crate::identity::FileSpan> {
        self.text.get(range.start..range.end)?;
        Some(crate::identity::FileSpan {
            file: self.origin,
            revision: self.revision,
            range,
        })
    }

    pub fn position(
        &self,
        offset: usize,
        encoding: crate::line_index::PositionEncoding,
    ) -> Option<crate::line_index::Position> {
        self.lines.position(&self.text, offset, encoding)
    }

    pub fn offset(
        &self,
        position: crate::line_index::Position,
        encoding: crate::line_index::PositionEncoding,
    ) -> Option<usize> {
        self.lines.offset(&self.text, position, encoding)
    }

    pub(crate) fn with_identity(
        mut self,
        id: crate::identity::FileId,
        revision: crate::identity::Revision,
        module: crate::identity::ModuleIdentity,
    ) -> Self {
        self.id = id;
        self.origin = id;
        self.revision = revision;
        self.module = module;
        self
    }
}
