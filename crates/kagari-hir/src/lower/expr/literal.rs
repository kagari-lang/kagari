//! Lexical literal preservation; numeric range/type validation belongs to type checking.

use kagari_syntax::{ast::expr::Literal as AstLiteral, kind::SyntaxKind};

use crate::{
    hir::expr::literal::{Literal, LiteralKind},
    lower::context::Lowerer,
};

impl Lowerer {
    /// Retains category and token text without parsing a scalar value; malformed/missing categories fall back to number syntax.
    pub(crate) fn lower_literal(&self, literal: &AstLiteral) -> Literal {
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
