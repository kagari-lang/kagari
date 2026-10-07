//! Type spellings, generic arguments and qualified member grouping.
//!
//! Every entry opens a `TypeRef`, then selects its structural child or path form.
//! `(T)` leaves parentheses and an inner `TypeRef` directly inside that wrapper;
//! `(T,)` and `()` insert a `TupleType` child. Angle tokens remain separate here,
//! allowing nested generic closing delimiters without expression shift joining.
//! These handlers do not resolve names, validate trait bounds or infer types.
//! See [`crate::ast::ty`] and [`crate::ast::misc::GenericArgList`] for child layouts.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{kind::SyntaxKind, parser::core::Parser, token::TokenKind};

impl<'a> Parser<'a> {
    /// Enters a guarded type-spelling parse, retaining an empty wrapper after a missing type.
    pub(crate) fn parse_type_ref(&mut self) {
        self.with_nesting(Self::parse_type_ref_nested);
    }

    fn parse_type_ref_nested(&mut self) {
        self.start_node(SyntaxKind::TypeRef);
        self.bump_trivia();
        match self.current_kind() {
            Some(TokenKind::Bang) => self.bump(),
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
            Some(TokenKind::Lt) => self.parse_qualified_type(),
            _ => self.error_here(DiagnosticKind::ExpectedType),
        }
        self.finish_node();
    }

    fn parse_qualified_type(&mut self) {
        self.start_node(SyntaxKind::QualifiedType);
        self.bump();
        self.parse_type_ref();
        self.bump_trivia();
        self.expect(TokenKind::AsKw, DiagnosticKind::ExpectedType);
        self.parse_trait_ref();
        self.bump_trivia();
        self.expect(TokenKind::Gt, DiagnosticKind::ExpectedType);
        self.bump_trivia();
        self.expect(TokenKind::ColonColon, DiagnosticKind::ExpectedType);
        self.bump_trivia();
        self.parse_name();
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_arg_list();
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

    /// Emits comma-separated types without consuming the owner's parentheses.
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

    /// Retains angles, positional `TypeRef` children and named associated-type bindings.
    pub(crate) fn parse_generic_arg_list(&mut self) {
        self.start_node(SyntaxKind::GenericArgList);
        self.expect(TokenKind::Lt, DiagnosticKind::ExpectedType);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::Gt, TokenKind::Eof]) {
            if self.nth_nontrivia_kind(0) == Some(TokenKind::Ident)
                && self.nth_nontrivia_kind(1) == Some(TokenKind::Eq)
            {
                self.start_node(SyntaxKind::AssociatedTypeBinding);
                self.parse_name();
                self.bump_trivia();
                self.bump();
                self.parse_type_ref();
                self.finish_node();
            } else {
                self.parse_type_ref();
            }
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
