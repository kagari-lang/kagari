//! String decoding at the syntax boundary; numeric range/type checks remain deferred.

use kagari_source::literal::{
    decode_byte_char_literal, decode_byte_string_literal_with_cancellation,
    decode_string_literal_with_cancellation,
};
use kagari_syntax::{ast::expr::Literal as AstLiteral, kind::SyntaxKind};
use kagari_types::scalar::BuiltinType;
use smallvec::SmallVec;

use crate::{
    hir::expr::{
        ExprData, ExprKind,
        literal::{Literal, LiteralKind},
    },
    lower::context::{Lowerer, syntax_span},
};

impl Lowerer {
    /// Byte strings become ordinary array construction, never pooled scalar literals.
    pub(super) fn lower_literal_expr(&mut self, literal: &AstLiteral) -> ExprKind {
        if literal.kind() != Some(SyntaxKind::ByteString) {
            return ExprKind::Literal(self.lower_literal(literal));
        }
        let text = literal.text().unwrap_or_default();
        let Ok(decoded) = decode_byte_string_literal_with_cancellation(&text, &self.cancel) else {
            return ExprKind::Missing;
        };
        let bytes = decoded.expect("ByteString tokens have validated literal syntax");
        let mut elements = SmallVec::new();
        for value in bytes {
            if self.cancel.check().is_err() {
                return ExprKind::Missing;
            }
            elements.push(self.alloc_expr(
                syntax_span(literal),
                ExprData {
                    kind: ExprKind::Literal(Literal {
                        kind: LiteralKind::Byte(value),
                        text: String::new(),
                    }),
                },
            ));
        }
        ExprKind::Array {
            elements,
            element_type: Some(BuiltinType::U8),
        }
    }

    /// Decodes validated String/byte tokens once; other categories retain their source spelling.
    /// Malformed/missing categories fall back to number syntax for recovery.
    pub(crate) fn lower_literal(&self, literal: &AstLiteral) -> Literal {
        let text = literal.text().unwrap_or_default();
        let kind = match literal.kind() {
            Some(SyntaxKind::Float) => LiteralKind::Float,
            Some(SyntaxKind::String) => LiteralKind::String,
            Some(SyntaxKind::Char) => LiteralKind::Char,
            Some(SyntaxKind::ByteChar) => LiteralKind::Byte(
                decode_byte_char_literal(&text)
                    .expect("ByteChar tokens have validated literal syntax"),
            ),
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
