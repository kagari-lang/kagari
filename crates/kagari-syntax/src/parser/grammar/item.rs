use kagari_common::DiagnosticKind;

use crate::{kind::SyntaxKind, token::TokenKind};

use crate::parser::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn parse_root(&mut self) {
        self.start_node(SyntaxKind::SourceFile);
        self.bump_trivia();

        while !self.at(TokenKind::Eof) {
            if self.parse_top_level() {
                break;
            }
            self.bump_trivia();
        }

        if self.at(TokenKind::Eof) {
            self.bump();
        }

        self.finish_node();
    }

    fn parse_top_level(&mut self) -> bool {
        match self.current_kind() {
            Some(TokenKind::PubKw) => self.parse_public_item(),
            Some(TokenKind::ModKw) => self.parse_module(),
            Some(TokenKind::UseKw) => self.parse_use(),
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::ConstKw) => self.parse_const(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::ValKw | TokenKind::VarKw) => self.parse_binding_stmt(),
            Some(TokenKind::WhileKw) => self.parse_while_stmt(),
            Some(TokenKind::LoopKw) => self.parse_loop_stmt(),
            Some(TokenKind::ReturnKw | TokenKind::BreakKw | TokenKind::ContinueKw) => {
                self.error_here(DiagnosticKind::TopLevelControlFlowNotAllowed);
                self.start_node(SyntaxKind::Error);
                self.bump();
                self.bump_trivia();
                if self.at(TokenKind::Semi) {
                    self.bump();
                }
                self.finish_node();
            }
            Some(TokenKind::Ident) if self.expr_followed_by_assignment() => {
                self.parse_assign_stmt()
            }
            Some(TokenKind::Ident) if self.current_text_is("let") => {
                self.recover_until_statement_boundary(DiagnosticKind::LegacyLetBinding);
            }
            Some(TokenKind::Ident) if self.current_text_is("static") => {
                self.recover_until_statement_boundary(DiagnosticKind::LegacyStaticItem);
            }
            Some(_) if self.expr_starts() => return self.parse_top_level_expr_stmt_or_tail(),
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

        false
    }

    fn parse_public_item(&mut self) {
        match self.nth_nontrivia_kind(1) {
            Some(TokenKind::ModKw) => self.parse_module(),
            Some(TokenKind::UseKw) => self.parse_use(),
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::ConstKw) => self.parse_const(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::Ident) if self.nth_nontrivia_text(1) == Some("static") => {
                self.recover_until_statement_boundary(DiagnosticKind::LegacyStaticItem);
            }
            _ => {
                self.error_here(DiagnosticKind::ExpectedTopLevelItem);
                self.bump_as_error();
            }
        }
    }

    fn parse_module_item(&mut self) {
        match self.current_kind() {
            Some(TokenKind::PubKw) => self.parse_public_item(),
            Some(TokenKind::ModKw) => self.parse_module(),
            Some(TokenKind::UseKw) => self.parse_use(),
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::ConstKw) => self.parse_const(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::Ident) if self.current_text_is("static") => {
                self.recover_until_statement_boundary(DiagnosticKind::LegacyStaticItem);
            }
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

    fn parse_module(&mut self) {
        self.start_node(SyntaxKind::ModuleDef);
        self.bump_trivia();
        if self.at(TokenKind::PubKw) {
            self.bump();
        }
        self.expect(TokenKind::ModKw, DiagnosticKind::ExpectedModuleKeyword);
        self.parse_module_name();
        self.bump_trivia();

        if self.at(TokenKind::Semi) {
            self.bump();
        } else {
            self.parse_module_block();
        }
        self.finish_node();
    }

    fn parse_module_block(&mut self) {
        self.start_node(SyntaxKind::ModuleBlock);
        if !self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart) {
            self.finish_node();
            return;
        }

        self.bump_trivia();
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.parse_module_item();
            self.bump_trivia();
        }

        self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        self.finish_node();
    }

    fn parse_use(&mut self) {
        self.start_node(SyntaxKind::UseDecl);
        self.bump_trivia();
        if self.at(TokenKind::PubKw) {
            self.bump();
        }
        self.expect(TokenKind::UseKw, DiagnosticKind::ExpectedUseKeyword);
        self.parse_use_tree();
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_use_tree(&mut self) {
        self.start_node(SyntaxKind::UseTree);
        self.bump_trivia();

        if self.at(TokenKind::LBrace) {
            self.parse_use_tree_group();
            self.finish_node();
            return;
        }

        if self.path_starts() {
            self.parse_path();
            self.bump_trivia();
            if self.at(TokenKind::AsKw) {
                self.bump();
                self.parse_use_alias();
            } else if self.at(TokenKind::ColonColon) {
                self.bump();
                self.bump_trivia();
                if self.at(TokenKind::Star) {
                    self.bump();
                } else if self.at(TokenKind::LBrace) {
                    self.parse_use_tree_group();
                } else {
                    self.error_here(DiagnosticKind::ExpectedUseTree);
                }
            }
        } else {
            self.error_here(DiagnosticKind::ExpectedUseTree);
        }

        self.finish_node();
    }

    fn parse_use_tree_group(&mut self) {
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedUseTree);
        self.start_node(SyntaxKind::UseTreeList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.parse_use_tree();
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
    }

    fn parse_function(&mut self) {
        self.start_node(SyntaxKind::FnDef);
        self.bump_trivia();
        if self.at(TokenKind::PubKw) {
            self.bump();
        }
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

    fn parse_const(&mut self) {
        self.start_node(SyntaxKind::ConstDef);
        self.bump_trivia();
        if self.at(TokenKind::PubKw) {
            self.bump();
        }
        self.expect(TokenKind::ConstKw, DiagnosticKind::ExpectedConstKeyword);
        self.parse_const_name();
        self.bump_trivia();
        if self.at(TokenKind::Colon) {
            self.bump();
            self.parse_type_ref();
        }
        self.expect(TokenKind::Eq, DiagnosticKind::ExpectedConstInitializer);
        self.parse_expr();
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_struct(&mut self) {
        self.start_node(SyntaxKind::StructDef);
        self.bump_trivia();
        if self.at(TokenKind::PubKw) {
            self.bump();
        }
        self.expect(TokenKind::StructKw, DiagnosticKind::ExpectedStructKeyword);
        self.parse_struct_name();
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart);

        self.start_node(SyntaxKind::FieldList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::Field);
            self.bump_trivia();
            if self.at(TokenKind::PubKw) {
                self.bump();
                self.bump_trivia();
            }
            if self.at_any(&[TokenKind::ValKw, TokenKind::VarKw]) {
                self.bump();
            } else {
                self.error_here(DiagnosticKind::ExpectedFieldBinding);
            }
            self.bump_trivia();
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
        self.bump_trivia();
        if self.at(TokenKind::PubKw) {
            self.bump();
        }
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

    pub(crate) fn parse_module_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedModuleName);
        self.finish_node();
    }

    pub(crate) fn parse_const_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedConstName);
        self.finish_node();
    }

    pub(crate) fn parse_enum_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedEnumName);
        self.finish_node();
    }

    pub(crate) fn parse_binding_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedBindingName);
        self.finish_node();
    }

    pub(crate) fn parse_use_alias(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedUseAlias);
        self.finish_node();
    }

    pub(crate) fn path_starts(&self) -> bool {
        matches!(
            self.current_kind(),
            Some(TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw)
        )
    }

    pub(crate) fn parse_path(&mut self) {
        self.start_node(SyntaxKind::Path);
        self.parse_path_segment();

        loop {
            if self.nth_nontrivia_kind(0) != Some(TokenKind::ColonColon)
                || !self.nth_nontrivia_is_path_segment(1)
            {
                break;
            }
            self.bump_trivia();
            self.bump();
            self.parse_path_segment();
        }

        self.finish_node();
    }

    fn nth_nontrivia_is_path_segment(&self, n: usize) -> bool {
        matches!(
            self.nth_nontrivia_kind(n),
            Some(TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw)
        )
    }

    fn parse_path_segment(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.bump_trivia();
        if self.path_starts() {
            self.bump();
        } else {
            self.error_here(DiagnosticKind::ExpectedPath);
        }
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
            self.bump_trivia();
            if self.current_text_is("ref") {
                self.recover_until_param_boundary(DiagnosticKind::LegacyRefParameter);
                self.finish_node();
                if self.at(TokenKind::Comma) {
                    self.bump();
                    self.bump_trivia();
                    continue;
                }
                break;
            }
            if self.current_text_is("mut") {
                self.recover_until_param_boundary(DiagnosticKind::LegacyReceiverModifier);
                self.finish_node();
                if self.at(TokenKind::Comma) {
                    self.bump();
                    self.bump_trivia();
                    continue;
                }
                break;
            }
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

    fn parse_top_level_expr_stmt_or_tail(&mut self) -> bool {
        let checkpoint = self.checkpoint();
        self.parse_expr();
        self.bump_trivia();

        if self.at(TokenKind::Semi) {
            self.finish_expr_stmt(checkpoint);
            return false;
        }

        if self.at(TokenKind::Eof) {
            return true;
        }

        self.error_here(DiagnosticKind::ExpectedStatementTerminator);
        false
    }
}
