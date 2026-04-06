use kagari_syntax::ast;

use crate::hir::{PatternData, PatternId, PatternKind};
use crate::lower::context::{Lowerer, syntax_span};

impl Lowerer {
    pub(crate) fn lower_pattern(&mut self, pattern: &ast::Pattern) -> PatternId {
        let span = syntax_span(pattern);
        let kind = if pattern.is_wildcard() {
            PatternKind::Wildcard
        } else if let Some(path) = pattern.path() {
            PatternKind::Name {
                name: path.name_text().unwrap_or_default(),
                local: self.alloc_local_id(span),
            }
        } else if let Some(literal) = pattern.literal() {
            PatternKind::Literal(self.lower_literal(&literal))
        } else {
            PatternKind::Name {
                name: "<missing>".to_string(),
                local: self.alloc_local_id(span),
            }
        };

        self.alloc_pattern(span, PatternData { kind })
    }
}
