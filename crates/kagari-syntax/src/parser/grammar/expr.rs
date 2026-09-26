use kagari_common::DiagnosticKind;

use crate::{kind::SyntaxKind, token::TokenKind};

use crate::parser::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn expr_starts(&self) -> bool {
        matches!(
            self.current_kind(),
            Some(
                TokenKind::Ident
                    | TokenKind::Lt
                    | TokenKind::CrateKw
                    | TokenKind::SelfKw
                    | TokenKind::SuperKw
                    | TokenKind::Number
                    | TokenKind::Float
                    | TokenKind::String
                    | TokenKind::TrueKw
                    | TokenKind::FalseKw
                    | TokenKind::IfKw
                    | TokenKind::MatchKw
                    | TokenKind::LoopKw
                    | TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::Minus
                    | TokenKind::Bang
                    | TokenKind::Pipe
                    | TokenKind::PipePipe
                    | TokenKind::LBrace
            )
        )
    }

    pub(crate) fn parse_expr(&mut self) {
        self.with_nesting(Self::parse_expr_nested);
    }

    fn parse_expr_nested(&mut self) {
        self.bump_trivia();
        self.parse_range_expr();
    }

    fn parse_range_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_logical_or_expr();
        self.bump_trivia();
        if self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
            self.bump();
            self.parse_logical_or_expr();
            self.start_node_at(checkpoint, SyntaxKind::RangeExpr);
            self.finish_node();
        }
    }

    pub(crate) fn parse_condition_expr(&mut self) {
        self.with_struct_literals_allowed(false, |parser| parser.parse_expr());
    }

    pub(crate) fn parse_condition(&mut self) {
        self.bump_trivia();
        if self.at(TokenKind::ValKw) {
            self.start_node(SyntaxKind::BindingCondition);
            self.bump();
            self.bump_trivia();
            self.parse_match_pattern();
            self.bump_trivia();
            self.expect(TokenKind::Eq, DiagnosticKind::UnexpectedToken);
            self.parse_condition_expr();
            self.finish_node();
        } else {
            self.parse_condition_expr();
        }
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
            if !self.at_any(&[TokenKind::Star, TokenKind::Slash, TokenKind::Percent]) {
                break;
            }
            self.bump();
            self.parse_prefix_expr();
            self.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
            self.finish_node();
        }
    }

    fn parse_prefix_expr(&mut self) {
        self.with_nesting(Self::parse_prefix_expr_nested);
    }

    fn parse_prefix_expr_nested(&mut self) {
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
            Some(TokenKind::Lt) => {
                self.start_node(SyntaxKind::PathExpr);
                self.parse_type_ref();
                self.finish_node();
            }
            Some(
                TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw,
            ) => self.parse_path_or_struct_expr(),
            Some(
                TokenKind::Number
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::TrueKw
                | TokenKind::FalseKw,
            ) => self.parse_literal(),
            Some(TokenKind::IfKw) => self.parse_if_expr(),
            Some(TokenKind::MatchKw) => self.parse_match_expr(),
            Some(TokenKind::LoopKw) => self.parse_loop_expr(),
            Some(TokenKind::LParen) => self.parse_paren_or_tuple_expr(),
            Some(TokenKind::LBracket) => self.parse_array_expr(),
            Some(TokenKind::LBrace) => self.parse_block(),
            Some(TokenKind::Pipe | TokenKind::PipePipe) => self.parse_closure_expr(),
            _ => self.error_here(DiagnosticKind::ExpectedExpression),
        }
    }

    fn parse_closure_expr(&mut self) {
        self.start_node(SyntaxKind::ClosureExpr);
        if self.at(TokenKind::PipePipe) {
            self.bump();
        } else {
            self.expect(TokenKind::Pipe, DiagnosticKind::ExpectedExpression);
            self.start_node(SyntaxKind::ClosureParamList);
            self.bump_trivia();
            while !self.at_any(&[TokenKind::Pipe, TokenKind::Eof]) {
                self.start_node(SyntaxKind::ClosureParam);
                self.parse_name();
                self.bump_trivia();
                if self.at(TokenKind::Colon) {
                    self.bump();
                    self.parse_type_ref();
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
            self.expect(TokenKind::Pipe, DiagnosticKind::ExpectedExpression);
        }
        self.bump_trivia();
        if self.at(TokenKind::LBrace) {
            self.parse_block();
        } else {
            self.parse_expr();
        }
        self.finish_node();
    }

    fn parse_path_or_struct_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_path_expr();
        self.bump_trivia();

        if self.allow_struct_literals() && self.at(TokenKind::Lt) {
            let mut cursor = self.cursor();
            self.nth_nontrivia_kind_from(&mut cursor);
            if self.skip_angle_group(&mut cursor)
                && self.nth_nontrivia_kind_from(&mut cursor) == Some(TokenKind::LBrace)
            {
                self.parse_generic_arg_list();
                self.bump_trivia();
            }
        }

        if self.allow_struct_literals() && self.at(TokenKind::LBrace) {
            self.parse_struct_literal_body();
            self.start_node_at(checkpoint, SyntaxKind::StructExpr);
            self.finish_node();
        }
    }

    fn parse_path_expr(&mut self) {
        self.start_node(SyntaxKind::PathExpr);
        self.parse_path();
        let mut cursor = self.cursor();
        if self.nth_nontrivia_kind_from(&mut cursor) == Some(TokenKind::Lt)
            && self.skip_angle_group(&mut cursor)
            && self.nth_nontrivia_kind_from(&mut cursor) == Some(TokenKind::ColonColon)
        {
            self.bump_trivia();
            self.parse_generic_arg_list();
            self.bump_trivia();
            self.expect(TokenKind::ColonColon, DiagnosticKind::ExpectedPath);
            self.bump_trivia();
            self.parse_variant_name();
        }
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
        self.with_nesting(Self::parse_if_expr_nested);
    }

    fn parse_if_expr_nested(&mut self) {
        self.start_node(SyntaxKind::IfExpr);
        self.expect(TokenKind::IfKw, DiagnosticKind::ExpectedIfKeyword);
        self.parse_condition();
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
            if self.at(TokenKind::IfKw) {
                self.bump();
                self.parse_expr();
                self.bump_trivia();
            }
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

    pub(crate) fn parse_match_pattern(&mut self) {
        self.with_nesting(Self::parse_match_pattern_nested);
    }

    fn parse_match_pattern_nested(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_single_pattern();
        self.bump_trivia();
        if self.at(TokenKind::Pipe) {
            while self.at(TokenKind::Pipe) {
                self.bump();
                self.bump_trivia();
                self.parse_single_pattern();
                self.bump_trivia();
            }
            self.start_node_at(checkpoint, SyntaxKind::Pattern);
            self.finish_node();
        }
    }

    fn parse_single_pattern(&mut self) {
        self.start_node(SyntaxKind::Pattern);
        match self.current_kind() {
            Some(
                TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw,
            ) => {
                self.parse_path_expr();
                self.bump_trivia();
                if self.at(TokenKind::LParen) {
                    self.parse_tuple_pattern();
                } else if self.at(TokenKind::LBrace) {
                    self.parse_struct_pattern();
                }
            }
            Some(
                TokenKind::Number
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::TrueKw
                | TokenKind::FalseKw,
            ) => self.parse_literal(),
            Some(TokenKind::LParen) => self.parse_tuple_pattern(),
            _ => self.error_here(DiagnosticKind::ExpectedMatchPattern),
        }
        self.bump_trivia();
        if self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
            self.bump();
            self.bump_trivia();
            match self.current_kind() {
                Some(
                    TokenKind::Number
                    | TokenKind::Float
                    | TokenKind::String
                    | TokenKind::TrueKw
                    | TokenKind::FalseKw,
                ) => self.parse_literal(),
                Some(
                    TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw,
                ) => self.parse_path_expr(),
                _ => self.error_here(DiagnosticKind::ExpectedMatchPattern),
            }
        }
        self.finish_node();
    }

    fn parse_tuple_pattern(&mut self) {
        self.expect(TokenKind::LParen, DiagnosticKind::ExpectedClosingParen);
        self.bump_trivia();

        while !self.at_any(&[TokenKind::RParen, TokenKind::Eof]) {
            self.parse_match_pattern();
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

    fn parse_loop_expr(&mut self) {
        self.start_node(SyntaxKind::LoopExpr);
        self.expect(TokenKind::LoopKw, DiagnosticKind::ExpectedLoopKeyword);
        self.bump_trivia();
        self.parse_block();
        self.finish_node();
    }

    fn parse_struct_pattern(&mut self) {
        self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedBlockEnd);
        self.bump_trivia();
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.start_node(SyntaxKind::PatternField);
            self.parse_field_name();
            self.bump_trivia();
            if self.at(TokenKind::Colon) {
                self.bump();
                self.bump_trivia();
                self.parse_match_pattern();
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
        self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
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
            self.bump_trivia();
            if self.at(TokenKind::Colon) {
                self.bump();
                self.parse_expr();
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
    }
}
