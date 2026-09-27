use crate::{
    Span,
    identity::{FileId, FileSpan, ModuleIdentity, Revision},
    line_index::{LineIndex, Position, PositionEncoding},
};
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
        Some(FileSpan {
            file: self.origin,
            revision: self.revision,
            range,
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
