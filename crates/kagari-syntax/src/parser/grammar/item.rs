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

        self.finish_remaining_tokens();
        self.finish_node();
    }

    fn parse_top_level(&mut self) -> bool {
        match self.current_kind() {
            Some(TokenKind::At) => self.parse_attributed_item(),
            Some(TokenKind::PubKw) => self.parse_public_item(),
            Some(TokenKind::ModKw) => self.parse_module(),
            Some(TokenKind::UseKw) => self.parse_use(),
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::ConstKw) => self.parse_const(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::TraitKw) => self.parse_trait(),
            Some(TokenKind::ImplKw) => self.parse_impl(),
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
        let offset = if self.nth_nontrivia_kind(1) == Some(TokenKind::LParen) {
            4
        } else {
            1
        };
        match self.nth_nontrivia_kind(offset) {
            Some(TokenKind::ModKw) => self.parse_module(),
            Some(TokenKind::UseKw) => self.parse_use(),
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::ConstKw) => self.parse_const(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::TraitKw) => self.parse_trait(),
            _ => {
                self.error_here(DiagnosticKind::ExpectedTopLevelItem);
                self.bump_as_error();
            }
        }
    }

    fn parse_module_item(&mut self) {
        match self.current_kind() {
            Some(TokenKind::At) => self.parse_attributed_item(),
            Some(TokenKind::PubKw) => self.parse_public_item(),
            Some(TokenKind::ModKw) => self.parse_module(),
            Some(TokenKind::UseKw) => self.parse_use(),
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::ConstKw) => self.parse_const(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::TraitKw) => self.parse_trait(),
            Some(TokenKind::ImplKw) => self.parse_impl(),
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

    fn parse_attributed_item(&mut self) {
        match self.attributed_item_kind() {
            Some(TokenKind::ModKw) => self.parse_module(),
            Some(TokenKind::UseKw) => self.parse_use(),
            Some(TokenKind::FnKw) => self.parse_function(),
            Some(TokenKind::ConstKw) => self.parse_const(),
            Some(TokenKind::StructKw) => self.parse_struct(),
            Some(TokenKind::EnumKw) => self.parse_enum(),
            Some(TokenKind::TraitKw) => self.parse_trait(),
            Some(TokenKind::ImplKw) => self.parse_impl(),
            _ => {
                self.error_here(DiagnosticKind::ExpectedTopLevelItem);
                self.bump_as_error();
            }
        }
    }

    fn attributed_item_kind(&self) -> Option<TokenKind> {
        let mut cursor = self.cursor();
        let mut kind = self.nth_nontrivia_kind_from(&mut cursor)?;
        while kind == TokenKind::At {
            kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            if !matches!(
                kind,
                TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw
            ) {
                return None;
            }
            kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            while kind == TokenKind::ColonColon {
                kind = self.nth_nontrivia_kind_from(&mut cursor)?;
                if !matches!(
                    kind,
                    TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw
                ) {
                    return None;
                }
                kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            }
            if kind == TokenKind::LParen {
                let mut depth = 1usize;
                while depth > 0 {
                    kind = self.nth_nontrivia_kind_from(&mut cursor)?;
                    match kind {
                        TokenKind::LParen => depth += 1,
                        TokenKind::RParen => depth -= 1,
                        TokenKind::Eof => return None,
                        _ => {}
                    }
                }
                kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            }
        }
        if kind == TokenKind::PubKw {
            let next = self.nth_nontrivia_kind_from(&mut cursor)?;
            if next == TokenKind::LParen {
                self.nth_nontrivia_kind_from(&mut cursor)?;
                self.nth_nontrivia_kind_from(&mut cursor)?;
                self.nth_nontrivia_kind_from(&mut cursor)
            } else {
                Some(next)
            }
        } else {
            Some(kind)
        }
    }

    fn parse_attributes(&mut self) {
        self.bump_trivia();
        while self.at(TokenKind::At) {
            self.parse_attribute();
            self.bump_trivia();
        }
    }

    fn parse_attribute(&mut self) {
        self.start_node(SyntaxKind::Attribute);
        self.bump();
        self.bump_trivia();
        self.parse_path();
        self.bump_trivia();
        if self.at(TokenKind::LParen) {
            self.start_node(SyntaxKind::AttributeArgs);
            self.bump();
            self.bump_trivia();
            if !self.at(TokenKind::RParen) {
                self.parse_attribute_arg_list(TokenKind::RParen);
            }
            self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
            self.finish_node();
        }
        self.finish_node();
    }

    fn parse_attribute_arg_list(&mut self, end: TokenKind) {
        self.start_node(SyntaxKind::AttributeArgList);
        while !self.at_any(&[end.clone(), TokenKind::Eof]) {
            self.start_node(SyntaxKind::AttributeArg);
            self.bump_trivia();
            if self.at(TokenKind::Ident) && self.nth_nontrivia_kind(1) == Some(TokenKind::Eq) {
                self.parse_name();
                self.expect(TokenKind::Eq, DiagnosticKind::UnexpectedToken);
            }
            self.parse_attribute_value();
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
    }

    fn parse_attribute_value(&mut self) {
        self.with_nesting(|parser| {
            parser.start_node(SyntaxKind::AttributeValue);
            parser.bump_trivia();
            match parser.current_kind() {
                Some(
                    TokenKind::Number
                    | TokenKind::Float
                    | TokenKind::String
                    | TokenKind::TrueKw
                    | TokenKind::FalseKw,
                ) => {
                    parser.start_node(SyntaxKind::Literal);
                    parser.bump();
                    parser.finish_node();
                }
                Some(TokenKind::LBracket) => {
                    parser.bump();
                    parser.bump_trivia();
                    if !parser.at(TokenKind::RBracket) {
                        parser.parse_attribute_arg_list(TokenKind::RBracket);
                    }
                    parser.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
                }
                Some(
                    TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw,
                ) => parser.parse_path(),
                _ => parser.error_here(DiagnosticKind::ExpectedExpression),
            }
            parser.finish_node();
        });
    }

    fn parse_module(&mut self) {
        self.with_nesting(Self::parse_module_nested);
    }

    fn parse_visibility(&mut self) {
        self.bump_trivia();
        if !self.at(TokenKind::PubKw) {
            return;
        }
        self.bump();
        self.bump_trivia();
        if self.at(TokenKind::LParen) {
            self.bump();
            self.expect(TokenKind::SuperKw, DiagnosticKind::UnexpectedToken);
            self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
        }
        self.bump_trivia();
    }

    fn parse_module_nested(&mut self) {
        self.start_node(SyntaxKind::ModuleDef);
        self.parse_attributes();
        self.bump_trivia();
        self.parse_visibility();
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
        if !self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedModuleBodyStart) {
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
        self.parse_attributes();
        self.bump_trivia();
        self.parse_visibility();
        self.expect(TokenKind::UseKw, DiagnosticKind::ExpectedUseKeyword);
        self.parse_use_tree();
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_use_tree(&mut self) {
        self.with_nesting(Self::parse_use_tree_nested);
    }

    fn parse_use_tree_nested(&mut self) {
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
        self.parse_attributes();
        self.bump_trivia();
        self.parse_visibility();
        self.expect(TokenKind::FnKw, DiagnosticKind::ExpectedFunctionKeyword);
        self.parse_name();
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_param_list();
        }
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
        if self.at(TokenKind::WhereKw) {
            self.parse_where_clause();
        }
        self.bump_trivia();
        self.parse_block();
        self.finish_node();
    }

    fn parse_const(&mut self) {
        self.start_node(SyntaxKind::ConstDef);
        self.parse_attributes();
        self.bump_trivia();
        self.parse_visibility();
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
        self.parse_attributes();
        self.bump_trivia();
        self.parse_visibility();
        self.expect(TokenKind::StructKw, DiagnosticKind::ExpectedStructKeyword);
        self.parse_struct_name();
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_param_list();
        }
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedTraitBodyStart);

        self.start_node(SyntaxKind::FieldList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::Field);
            self.parse_attributes();
            self.bump_trivia();
            self.parse_visibility();
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
        self.parse_attributes();
        self.bump_trivia();
        self.parse_visibility();
        self.expect(TokenKind::EnumKw, DiagnosticKind::ExpectedEnumKeyword);
        self.parse_enum_name();
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_param_list();
        }
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedImplBodyStart);

        self.start_node(SyntaxKind::VariantList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::Variant);
            self.parse_variant_name();
            self.bump_trivia();
            if self.at(TokenKind::LParen) {
                self.bump();
                self.parse_type_list();
                self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
            }
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

    fn parse_trait(&mut self) {
        self.start_node(SyntaxKind::TraitDef);
        self.parse_attributes();
        self.bump_trivia();
        self.parse_visibility();
        self.expect(TokenKind::TraitKw, DiagnosticKind::ExpectedTraitKeyword);
        self.parse_trait_name();
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_param_list();
        }
        self.bump_trivia();
        if self.at(TokenKind::Colon) {
            self.bump();
            self.parse_trait_bound_list();
        }
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart);

        self.bump_trivia();
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            if self.attributed_item_kind() == Some(TokenKind::TypeKw) {
                self.parse_associated_type();
            } else if self.attributed_item_kind() == Some(TokenKind::ConstKw) {
                self.parse_associated_const();
            } else {
                self.parse_method(false);
            }
            self.bump_trivia();
        }

        self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        self.finish_node();
    }

    fn parse_impl(&mut self) {
        self.start_node(SyntaxKind::ImplBlock);
        self.parse_attributes();
        self.bump_trivia();
        self.expect(TokenKind::ImplKw, DiagnosticKind::ExpectedImplKeyword);
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_param_list();
        }
        self.bump_trivia();

        if self.impl_target_followed_by_for() {
            self.parse_trait_ref();
            self.bump_trivia();
            self.expect(TokenKind::ForKw, DiagnosticKind::ExpectedForKeyword);
            self.parse_type_ref();
        } else {
            self.parse_type_ref();
        }

        self.bump_trivia();
        if self.at(TokenKind::WhereKw) {
            self.parse_where_clause();
        }
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart);

        self.bump_trivia();
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            if self.attributed_item_kind() == Some(TokenKind::TypeKw) {
                self.parse_associated_type();
            } else if self.attributed_item_kind() == Some(TokenKind::ConstKw) {
                self.parse_associated_const();
            } else {
                self.parse_method(true);
            }
            self.bump_trivia();
        }

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

    pub(crate) fn parse_trait_name(&mut self) {
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedTraitName);
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

    fn parse_generic_param_list(&mut self) {
        self.start_node(SyntaxKind::GenericParamList);
        self.expect(TokenKind::Lt, DiagnosticKind::ExpectedGenericParameterName);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::Gt, TokenKind::Eof]) {
            self.start_node(SyntaxKind::GenericParam);
            self.start_node(SyntaxKind::Name);
            self.expect(
                TokenKind::Ident,
                DiagnosticKind::ExpectedGenericParameterName,
            );
            self.finish_node();
            self.bump_trivia();
            if self.at(TokenKind::Colon) {
                self.bump();
                self.parse_trait_bound_list();
            }
            self.finish_node();

            self.bump_trivia();
            if self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
            } else {
                break;
            }
        }

        self.expect(TokenKind::Gt, DiagnosticKind::ExpectedGenericParameterName);
        self.finish_node();
    }

    pub(crate) fn parse_trait_bound_list(&mut self) {
        self.start_node(SyntaxKind::TraitBoundList);
        self.parse_trait_ref();
        self.bump_trivia();

        while self.at(TokenKind::Plus) {
            self.bump();
            self.parse_trait_ref();
            self.bump_trivia();
        }

        self.finish_node();
    }

    pub(crate) fn parse_trait_ref(&mut self) {
        self.start_node(SyntaxKind::TraitRef);
        self.parse_path();
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_arg_list();
        }
        self.finish_node();
    }

    fn parse_associated_type(&mut self) {
        self.start_node(SyntaxKind::AssociatedType);
        self.parse_attributes();
        self.bump_trivia();
        self.expect(TokenKind::TypeKw, DiagnosticKind::ExpectedType);
        self.bump_trivia();
        self.parse_name();
        self.bump_trivia();
        if self.at(TokenKind::Colon) {
            self.bump();
            self.parse_trait_bound_list();
        }
        self.bump_trivia();
        if self.at(TokenKind::Eq) {
            self.bump();
            self.parse_type_ref();
        }
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_associated_const(&mut self) {
        self.start_node(SyntaxKind::ConstDef);
        self.parse_attributes();
        self.bump_trivia();
        self.expect(TokenKind::ConstKw, DiagnosticKind::ExpectedConstKeyword);
        self.parse_const_name();
        self.bump_trivia();
        self.expect(TokenKind::Colon, DiagnosticKind::ExpectedType);
        self.parse_type_ref();
        self.bump_trivia();
        if self.at(TokenKind::Eq) {
            self.bump();
            self.parse_expr();
        }
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_where_clause(&mut self) {
        self.start_node(SyntaxKind::WhereClause);
        self.expect(TokenKind::WhereKw, DiagnosticKind::UnexpectedToken);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::LBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::WherePredicate);
            self.parse_type_ref();
            self.expect(
                TokenKind::Colon,
                DiagnosticKind::ExpectedWherePredicateSeparator,
            );
            self.parse_trait_bound_list();
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

    fn impl_target_followed_by_for(&self) -> bool {
        let mut cursor = self.cursor();
        if !matches!(
            self.nth_nontrivia_kind_from(&mut cursor),
            Some(TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw)
        ) {
            return false;
        }

        loop {
            match self.nth_nontrivia_kind_from(&mut cursor) {
                Some(TokenKind::ColonColon) => {
                    if !matches!(
                        self.nth_nontrivia_kind_from(&mut cursor),
                        Some(
                            TokenKind::Ident
                                | TokenKind::CrateKw
                                | TokenKind::SelfKw
                                | TokenKind::SuperKw
                        )
                    ) {
                        return false;
                    }
                }
                Some(TokenKind::Lt) => {
                    if !self.skip_angle_group(&mut cursor) {
                        return false;
                    }
                }
                Some(TokenKind::ForKw) => return true,
                _ => return false,
            }
        }
    }

    pub(crate) fn skip_angle_group(&self, cursor: &mut usize) -> bool {
        let mut depth = 1;
        while let Some(kind) = self.nth_nontrivia_kind_from(cursor) {
            match kind {
                TokenKind::Lt => depth += 1,
                TokenKind::Gt => {
                    depth -= 1;
                    if depth == 0 {
                        return true;
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
        }

        false
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

    fn parse_method(&mut self, allow_visibility: bool) {
        self.start_node(SyntaxKind::MethodDef);
        self.parse_attributes();
        self.bump_trivia();
        if allow_visibility {
            self.parse_visibility();
        }
        self.expect(TokenKind::FnKw, DiagnosticKind::ExpectedFunctionKeyword);
        self.parse_name();
        self.bump_trivia();
        if self.at(TokenKind::Lt) {
            self.parse_generic_param_list();
        }
        self.expect(
            TokenKind::LParen,
            DiagnosticKind::ExpectedFunctionParameterListStart,
        );
        self.parse_method_param_list();
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
        if self.at(TokenKind::WhereKw) {
            self.parse_where_clause();
        }

        self.bump_trivia();
        if self.at(TokenKind::LBrace) {
            self.parse_block();
        } else {
            self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        }
        self.finish_node();
    }

    fn parse_param_list(&mut self) {
        self.start_node(SyntaxKind::ParamList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RParen, TokenKind::Eof]) {
            self.parse_param();

            if self.at(TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
            self.bump_trivia();
        }

        self.finish_node();
    }

    fn parse_method_param_list(&mut self) {
        self.start_node(SyntaxKind::ParamList);
        self.bump_trivia();
        let mut first = true;

        while !self.at_any(&[TokenKind::RParen, TokenKind::Eof]) {
            if first && self.at(TokenKind::SelfKw) {
                self.start_node(SyntaxKind::Param);
                self.start_node(SyntaxKind::Name);
                self.bump();
                self.finish_node();
                self.finish_node();
            } else {
                self.parse_param();
            }
            first = false;

            if self.at(TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
            self.bump_trivia();
        }

        self.finish_node();
    }

    fn parse_param(&mut self) {
        self.start_node(SyntaxKind::Param);
        self.bump_trivia();
        self.parse_parameter_name();
        self.expect(
            TokenKind::Colon,
            DiagnosticKind::ExpectedParameterTypeSeparator,
        );
        self.parse_type_ref();
        self.finish_node();
    }
}
