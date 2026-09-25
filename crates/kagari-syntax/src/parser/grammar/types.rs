use kagari_common::DiagnosticKind;

use crate::{kind::SyntaxKind, token::TokenKind};

use crate::parser::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn parse_type_ref(&mut self) {
        self.with_nesting(Self::parse_type_ref_nested);
    }

    fn parse_type_ref_nested(&mut self) {
        self.start_node(SyntaxKind::TypeRef);
        self.bump_trivia();
        match self.current_kind() {
            Some(
                TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw,
            ) => {
                self.parse_path();
                if self.nth_nontrivia_kind(0) == Some(TokenKind::Lt) {
                    self.bump_trivia();
                    self.parse_generic_arg_list();
                }
            }
            Some(TokenKind::LParen) => self.parse_paren_or_tuple_type(),
            Some(TokenKind::LBracket) => self.parse_array_type(),
            Some(TokenKind::FnKw) => self.parse_function_type(),
            _ => self.error_here(DiagnosticKind::ExpectedType),
        }
        self.finish_node();
    }

    fn parse_function_type(&mut self) {
        self.start_node(SyntaxKind::FunctionType);
        self.bump();
        self.bump_trivia();
        self.expect(TokenKind::LParen, DiagnosticKind::ExpectedClosingParen);
        self.parse_type_list();
        self.bump_trivia();
        self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
        self.bump_trivia();
        self.expect(TokenKind::Arrow, DiagnosticKind::ExpectedType);
        self.parse_type_ref();
        self.finish_node();
    }

    fn parse_paren_or_tuple_type(&mut self) {
        let checkpoint = self.checkpoint();
        self.expect(TokenKind::LParen, DiagnosticKind::ExpectedClosingParen);
        self.bump_trivia();
        if self.at(TokenKind::RParen) {
            self.bump();
            self.start_node_at(checkpoint, SyntaxKind::TupleType);
            self.finish_node();
            return;
        }

        self.parse_type_ref();
        self.bump_trivia();
        if self.at(TokenKind::Comma) {
            while self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
                if self.at(TokenKind::RParen) {
                    break;
                }
                self.parse_type_ref();
                self.bump_trivia();
            }
            self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
            self.start_node_at(checkpoint, SyntaxKind::TupleType);
            self.finish_node();
        } else {
            self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
        }
    }

    fn parse_array_type(&mut self) {
        self.start_node(SyntaxKind::ArrayType);
        self.expect(TokenKind::LBracket, DiagnosticKind::ExpectedClosingBracket);
        self.parse_type_ref();
        self.bump_trivia();
        self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
        self.finish_node();
    }

    pub(crate) fn parse_type_list(&mut self) {
        self.start_node(SyntaxKind::TypeList);
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

        self.finish_node();
    }

    pub(crate) fn parse_generic_arg_list(&mut self) {
        self.start_node(SyntaxKind::GenericArgList);
        self.expect(TokenKind::Lt, DiagnosticKind::ExpectedType);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::Gt, TokenKind::Eof]) {
            self.parse_type_ref();
            self.bump_trivia();
            if self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
            } else {
                break;
            }
        }

        self.expect(TokenKind::Gt, DiagnosticKind::ExpectedType);
        self.finish_node();
    }
}
