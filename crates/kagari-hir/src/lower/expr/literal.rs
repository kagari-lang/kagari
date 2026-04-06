use kagari_syntax::ast;
use kagari_syntax::kind::SyntaxKind;

use crate::hir::{Literal, LiteralKind};
use crate::lower::context::Lowerer;

impl Lowerer {
    pub(crate) fn lower_literal(&self, literal: &ast::Literal) -> Literal {
        let text = literal.text().unwrap_or_default();
        let kind = match literal.kind() {
            Some(SyntaxKind::Float) => LiteralKind::Float,
            Some(SyntaxKind::String) => LiteralKind::String,
            Some(SyntaxKind::TrueKw | SyntaxKind::FalseKw) => LiteralKind::Bool,
            _ => LiteralKind::Number,
        };

        Literal { kind, text }
    }
}
