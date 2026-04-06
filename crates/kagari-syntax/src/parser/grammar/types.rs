use kagari_common::DiagnosticKind;

use crate::{kind::SyntaxKind, token::TokenKind};

use crate::parser::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn parse_type_ref(&mut self) {
        self.start_node(SyntaxKind::TypeRef);
        self.bump_trivia();
        match self.current_kind() {
            Some(TokenKind::Ident) => self.parse_name(),
            Some(TokenKind::LParen) => self.parse_tuple_type(),
            Some(TokenKind::LBracket) => self.parse_array_type(),
            _ => self.error_here(DiagnosticKind::ExpectedType),
        }
        self.finish_node();
    }

    fn parse_tuple_type(&mut self) {
        self.start_node(SyntaxKind::TupleType);
        self.expect(TokenKind::LParen, DiagnosticKind::ExpectedClosingParen);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RParen, TokenKind::Eof]) {
            self.parse_type_ref();
            self.bump_trivia();
            if self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
            } else {
                break;
            }
        }

        self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
        self.finish_node();
    }

    fn parse_array_type(&mut self) {
        self.start_node(SyntaxKind::ArrayType);
        self.expect(TokenKind::LBracket, DiagnosticKind::ExpectedClosingBracket);
        self.parse_type_ref();
        self.bump_trivia();
        self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
        self.finish_node();
    }
}
