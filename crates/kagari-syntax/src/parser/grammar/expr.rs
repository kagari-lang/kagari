use kagari_common::DiagnosticKind;

use crate::{kind::SyntaxKind, token::TokenKind};

use crate::parser::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn expr_starts(&self) -> bool {
        matches!(
            self.current_kind(),
            Some(
                TokenKind::Ident
                    | TokenKind::Number
                    | TokenKind::Float
                    | TokenKind::String
                    | TokenKind::TrueKw
                    | TokenKind::FalseKw
                    | TokenKind::IfKw
                    | TokenKind::MatchKw
                    | TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::Minus
                    | TokenKind::Bang
            )
        )
    }

    pub(crate) fn expr_followed_by_assignment(&self) -> bool {
        let mut cursor = self.cursor();
        if self.nth_nontrivia_kind_from(&mut cursor) != Some(TokenKind::Ident) {
            return false;
        }

        loop {
            match self.nth_nontrivia_kind_from(&mut cursor) {
                Some(TokenKind::Eq) => return true,
                Some(TokenKind::Dot) => {
                    if self.nth_nontrivia_kind_from(&mut cursor) != Some(TokenKind::Ident) {
                        return false;
                    }
                }
                Some(TokenKind::LBracket) => {
                    if !self.skip_balanced_delimiters(&mut cursor, TokenKind::LBracket) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }

    pub(crate) fn parse_expr(&mut self) {
        self.bump_trivia();
        self.parse_logical_or_expr();
    }

    pub(crate) fn parse_condition_expr(&mut self) {
        self.with_struct_literals_allowed(false, |parser| parser.parse_expr());
    }

    pub(crate) fn parse_place_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_path_expr();

        loop {
            self.bump_trivia();
            match self.current_kind() {
                Some(TokenKind::Dot) => {
                    self.parse_field_suffix();
                    self.start_node_at(checkpoint, SyntaxKind::FieldExpr);
                    self.finish_node();
                }
                Some(TokenKind::LBracket) => {
                    self.parse_index_suffix();
                    self.start_node_at(checkpoint, SyntaxKind::IndexExpr);
                    self.finish_node();
                }
                _ => break,
            }
        }
    }

    fn skip_balanced_delimiters(&self, cursor: &mut usize, opening: TokenKind) -> bool {
        let mut stack = vec![opening];
        while let Some(kind) = self.nth_nontrivia_kind_from(cursor) {
            match kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => stack.push(kind),
                TokenKind::RParen => {
                    if !matches!(stack.pop(), Some(TokenKind::LParen)) {
                        return false;
                    }
                }
                TokenKind::RBracket => {
                    if !matches!(stack.pop(), Some(TokenKind::LBracket)) {
                        return false;
                    }
                    if stack.is_empty() {
                        return true;
                    }
                }
                TokenKind::RBrace => {
                    if !matches!(stack.pop(), Some(TokenKind::LBrace)) {
                        return false;
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
        }

        false
    }

    fn parse_logical_or_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_logical_and_expr();

        loop {
            self.bump_trivia();
            if !self.at(TokenKind::PipePipe) {
                break;
            }
            self.bump();
            self.parse_logical_and_expr();
            self.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
            self.finish_node();
        }
    }

    fn parse_logical_and_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_equality_expr();

        loop {
            self.bump_trivia();
            if !self.at(TokenKind::AmpAmp) {
                break;
            }
            self.bump();
            self.parse_equality_expr();
            self.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
            self.finish_node();
        }
    }

    fn parse_equality_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_comparison_expr();

        loop {
            self.bump_trivia();
            if !self.at_any(&[TokenKind::EqEq, TokenKind::NotEq]) {
                break;
            }
            self.bump();
            self.parse_comparison_expr();
            self.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
            self.finish_node();
        }
    }

    fn parse_comparison_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_additive_expr();

        loop {
            self.bump_trivia();
            if !self.at_any(&[TokenKind::Lt, TokenKind::Gt, TokenKind::Le, TokenKind::Ge]) {
                break;
            }
            self.bump();
            self.parse_additive_expr();
            self.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
            self.finish_node();
        }
    }

    fn parse_additive_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_multiplicative_expr();

        loop {
            self.bump_trivia();
            if !self.at_any(&[TokenKind::Plus, TokenKind::Minus]) {
                break;
            }
            self.bump();
            self.parse_multiplicative_expr();
            self.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
            self.finish_node();
        }
    }

    fn parse_multiplicative_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_prefix_expr();

        loop {
            self.bump_trivia();
            if !self.at_any(&[TokenKind::Star, TokenKind::Slash]) {
                break;
            }
            self.bump();
            self.parse_prefix_expr();
            self.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
            self.finish_node();
        }
    }

    fn parse_prefix_expr(&mut self) {
        self.bump_trivia();
        if self.at_any(&[TokenKind::Minus, TokenKind::Bang]) {
            let checkpoint = self.checkpoint();
            self.bump();
            self.parse_prefix_expr();
            self.start_node_at(checkpoint, SyntaxKind::PrefixExpr);
            self.finish_node();
            return;
        }

        self.parse_postfix_expr();
    }

    fn parse_postfix_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_atom();

        loop {
            self.bump_trivia();
            match self.current_kind() {
                Some(TokenKind::LParen) => {
                    self.parse_call_suffix();
                    self.start_node_at(checkpoint, SyntaxKind::CallExpr);
                    self.finish_node();
                }
                Some(TokenKind::Dot) => {
                    self.parse_field_suffix();
                    self.start_node_at(checkpoint, SyntaxKind::FieldExpr);
                    self.finish_node();
                }
                Some(TokenKind::LBracket) => {
                    self.parse_index_suffix();
                    self.start_node_at(checkpoint, SyntaxKind::IndexExpr);
                    self.finish_node();
                }
                _ => break,
            }
        }
    }

    fn parse_atom(&mut self) {
        self.bump_trivia();
        match self.current_kind() {
            Some(TokenKind::Ident) => self.parse_path_or_struct_expr(),
            Some(
                TokenKind::Number
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::TrueKw
                | TokenKind::FalseKw,
            ) => self.parse_literal(),
            Some(TokenKind::IfKw) => self.parse_if_expr(),
            Some(TokenKind::MatchKw) => self.parse_match_expr(),
            Some(TokenKind::LParen) => self.parse_paren_or_tuple_expr(),
            Some(TokenKind::LBracket) => self.parse_array_expr(),
            _ => self.error_here(DiagnosticKind::ExpectedExpression),
        }
    }

    fn parse_path_or_struct_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_path_expr();
        self.bump_trivia();

        if self.allow_struct_literals() && self.at(TokenKind::LBrace) {
            self.parse_struct_literal_body();
            self.start_node_at(checkpoint, SyntaxKind::StructExpr);
            self.finish_node();
        }
    }

    fn parse_path_expr(&mut self) {
        self.start_node(SyntaxKind::PathExpr);
        self.parse_name();
        self.finish_node();
    }

    fn parse_literal(&mut self) {
        self.start_node(SyntaxKind::Literal);
        match self.current_kind() {
            Some(
                TokenKind::Number
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::TrueKw
                | TokenKind::FalseKw,
            ) => self.bump(),
            _ => self.error_here(DiagnosticKind::ExpectedExpression),
        }
        self.finish_node();
    }

    fn parse_paren_or_tuple_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.expect(TokenKind::LParen, DiagnosticKind::ExpectedExpression);
        self.bump_trivia();

        if self.at(TokenKind::RParen) {
            self.bump();
            self.start_node_at(checkpoint, SyntaxKind::TupleExpr);
            self.finish_node();
            return;
        }

        self.parse_expr();
        self.bump_trivia();

        if self.at(TokenKind::Comma) {
            while self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
                if self.at(TokenKind::RParen) {
                    break;
                }
                self.parse_expr();
                self.bump_trivia();
            }
            self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
            self.start_node_at(checkpoint, SyntaxKind::TupleExpr);
            self.finish_node();
            return;
        }

        self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
        self.start_node_at(checkpoint, SyntaxKind::ParenExpr);
        self.finish_node();
    }

    fn parse_array_expr(&mut self) {
        self.start_node(SyntaxKind::ArrayExpr);
        self.expect(TokenKind::LBracket, DiagnosticKind::ExpectedClosingBracket);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBracket, TokenKind::Eof]) {
            self.parse_expr();
            self.bump_trivia();
            if self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
            } else {
                break;
            }
        }

        self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
        self.finish_node();
    }

    fn parse_call_suffix(&mut self) {
        self.expect(
            TokenKind::LParen,
            DiagnosticKind::ExpectedFunctionParameterListStart,
        );
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RParen, TokenKind::Eof]) {
            self.parse_expr();
            self.bump_trivia();
            if self.at(TokenKind::Comma) {
                self.bump();
                self.bump_trivia();
            } else {
                break;
            }
        }

        self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
    }

    fn parse_field_suffix(&mut self) {
        self.expect(TokenKind::Dot, DiagnosticKind::UnexpectedToken);
        self.start_node(SyntaxKind::Name);
        self.expect(TokenKind::Ident, DiagnosticKind::ExpectedFieldName);
        self.finish_node();
    }

    fn parse_index_suffix(&mut self) {
        self.expect(TokenKind::LBracket, DiagnosticKind::UnexpectedToken);
        self.parse_expr();
        self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
    }

    fn parse_if_expr(&mut self) {
        self.start_node(SyntaxKind::IfExpr);
        self.expect(TokenKind::IfKw, DiagnosticKind::ExpectedIfKeyword);
        self.parse_condition_expr();
        self.bump_trivia();
        self.parse_block();
        self.bump_trivia();

        if self.at(TokenKind::ElseKw) {
            self.bump();
            self.bump_trivia();
            match self.current_kind() {
                Some(TokenKind::IfKw) => self.parse_if_expr(),
                Some(TokenKind::LBrace) => self.parse_block(),
                _ => self.error_here(DiagnosticKind::ExpectedElseBranch),
            }
        }

        self.finish_node();
    }

    fn parse_match_expr(&mut self) {
        self.start_node(SyntaxKind::MatchExpr);
        self.expect(TokenKind::MatchKw, DiagnosticKind::ExpectedMatchKeyword);
        self.parse_condition_expr();
        self.bump_trivia();
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedMatchBodyStart);

        self.start_node(SyntaxKind::MatchArmList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::MatchArm);
            self.parse_match_pattern();
            self.bump_trivia();
            self.expect(TokenKind::FatArrow, DiagnosticKind::ExpectedMatchArmArrow);
            self.parse_expr();
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

    fn parse_match_pattern(&mut self) {
        self.start_node(SyntaxKind::Pattern);
        match self.current_kind() {
            Some(TokenKind::Ident) => self.parse_path_expr(),
            Some(
                TokenKind::Number
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::TrueKw
                | TokenKind::FalseKw,
            ) => self.parse_literal(),
            _ => self.error_here(DiagnosticKind::ExpectedMatchPattern),
        }
        self.finish_node();
    }

    fn parse_struct_literal_body(&mut self) {
        self.expect(
            TokenKind::LBrace,
            DiagnosticKind::ExpectedStructLiteralBodyStart,
        );
        self.start_node(SyntaxKind::FieldInitList);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::FieldInit);
            self.parse_field_name();
            self.expect(TokenKind::Colon, DiagnosticKind::ExpectedFieldTypeSeparator);
            self.parse_expr();
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
    }
}
