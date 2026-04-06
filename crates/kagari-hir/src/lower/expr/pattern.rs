use kagari_syntax::ast;

use crate::hir::{PatternData, PatternKind};
use crate::lower::context::{Lowerer, syntax_span};

impl Lowerer {
    pub(crate) fn lower_pattern(&mut self, pattern: &ast::Pattern) -> crate::hir::PatternId {
        let kind = if pattern.is_wildcard() {
            PatternKind::Wildcard
        } else if let Some(path) = pattern.path() {
            PatternKind::Name(path.name_text().unwrap_or_default())
        } else if let Some(literal) = pattern.literal() {
            PatternKind::Literal(self.lower_literal(&literal))
        } else {
            PatternKind::Name("<missing>".to_string())
        };

        self.alloc_pattern(syntax_span(pattern), PatternData { kind })
    }
}
