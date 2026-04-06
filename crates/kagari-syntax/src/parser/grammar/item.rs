use kagari_common::DiagnosticKind;

use crate::{kind::SyntaxKind, token::TokenKind};

use crate::parser::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn parse_root(&mut self) {
        self.start_node(SyntaxKind::SourceFile);
        self.bump_trivia();

        while !self.at(TokenKind::Eof) {
            self.parse_item();
            self.bump_trivia();
        }

        if self.at(TokenKind::Eof) {
            self.bump();
        }

        self.finish_node();
    }

    fn parse_item(&mut self) {
        match self.current_kind() {
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::Unknown) => {
                self.error_here(DiagnosticKind::UnexpectedToken);
                self.bump_as_error();
            }
            Some(_) => {
                self.error_here(DiagnosticKind::ExpectedTopLevelItem);
                self.bump_as_error();
            }
            None => {}
        }
    }

    fn parse_function(&mut self) {
        self.start_node(SyntaxKind::FnDef);
        self.expect(TokenKind::FnKw, DiagnosticKind::ExpectedFunctionKeyword);
        self.parse_name();
        self.expect(
            TokenKind::LParen,
            DiagnosticKind::ExpectedFunctionParameterListStart,
        );
        self.parse_param_list();
        self.expect(
            TokenKind::RParen,
            DiagnosticKind::ExpectedFunctionParameterListEnd,
        );

        self.bump_trivia();
        if self.at(TokenKind::Arrow) {
            self.bump();
            self.parse_type_ref();
        }

        self.bump_trivia();
        self.parse_block();
        self.finish_node();
    }

    fn parse_struct(&mut self) {
        self.start_node(SyntaxKind::StructDef);
        self.expect(TokenKind::StructKw, DiagnosticKind::ExpectedStructKeyword);
        self.parse_struct_name();
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart);

        self.start_node(SyntaxKind::FieldList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::Field);
            self.parse_field_name();
            self.expect(TokenKind::Colon, DiagnosticKind::ExpectedFieldTypeSeparator);
            self.parse_type_ref();
            self.finish_node();

            self.bump_trivia();
            if self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
            } else {
                break;
            }
        }

        self.finish_node();
        self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        self.finish_node();
    }

    fn parse_enum(&mut self) {
        self.start_node(SyntaxKind::EnumDef);
        self.expect(TokenKind::EnumKw, DiagnosticKind::ExpectedEnumKeyword);
        self.parse_enum_name();
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart);

        self.start_node(SyntaxKind::VariantList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::Variant);
            self.parse_variant_name();
            self.finish_node();

            self.bump_trivia();
            if self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
            } else {
                break;
            }
        }

        self.finish_node();
        self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        self.finish_node();
    }

    pub(crate) fn parse_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedFunctionName);
        self.finish_node();
    }

    pub(crate) fn parse_parameter_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedParameterName);
        self.finish_node();
    }

    pub(crate) fn parse_struct_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedStructName);
        self.finish_node();
    }

    pub(crate) fn parse_enum_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedEnumName);
        self.finish_node();
    }

    pub(crate) fn parse_let_binding_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedLetBindingName);
        self.finish_node();
    }

    pub(crate) fn parse_field_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedFieldName);
        self.finish_node();
    }

    pub(crate) fn parse_variant_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedVariantName);
        self.finish_node();
    }

    fn parse_param_list(&mut self) {
        self.start_node(SyntaxKind::ParamList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RParen, TokenKind::Eof]) {
            self.start_node(SyntaxKind::Param);
            self.parse_parameter_name();
            self.expect(
                TokenKind::Colon,
                DiagnosticKind::ExpectedParameterTypeSeparator,
            );
            self.parse_type_ref();
            self.finish_node();

            if self.at(TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
            self.bump_trivia();
        }

        self.finish_node();
    }
}
