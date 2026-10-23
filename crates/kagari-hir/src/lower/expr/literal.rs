//! String decoding at the syntax boundary; numeric range/type checks remain deferred.

use kagari_source::literal::decode_string_literal_with_cancellation;
use kagari_syntax::{ast::expr::Literal as AstLiteral, kind::SyntaxKind};

use crate::{
    hir::expr::literal::{Literal, LiteralKind},
    lower::context::Lowerer,
};

impl Lowerer {
    /// Decodes validated String tokens once; other categories retain their source spelling.
    /// Malformed/missing categories fall back to number syntax for recovery.
    pub(crate) fn lower_literal(&self, literal: &AstLiteral) -> Literal {
        let text = literal.text().unwrap_or_default();
        let kind = match literal.kind() {
            Some(SyntaxKind::Float) => LiteralKind::Float,
            Some(SyntaxKind::String) => LiteralKind::String,
            Some(SyntaxKind::Char) => LiteralKind::Char,
            Some(SyntaxKind::TrueKw | SyntaxKind::FalseKw) => LiteralKind::Bool,
            _ => LiteralKind::Number,
        };

        let text = if kind == LiteralKind::String {
            let Ok(decoded) = decode_string_literal_with_cancellation(&text, &self.cancel) else {
                // The enclosing controlled analysis rejects cancelled publication.
                return Literal {
                    kind: LiteralKind::Number,
                    text: String::new(),
                };
            };
            decoded.expect("String tokens have validated literal syntax")
        } else {
            text
        };
        Literal { kind, text }
    }
}
