use kagari_common::DiagnosticKind;

use crate::{kind::SyntaxKind, token::TokenKind};

use crate::parser::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn parse_block(&mut self) {
        self.start_node(SyntaxKind::BlockExpr);
        if !self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedFunctionBodyStart) {
            self.finish_node();
            return;
        }

        self.bump_trivia();
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            match self.current_kind() {
                Some(TokenKind::LetKw) => self.parse_let_stmt(),
                Some(TokenKind::ReturnKw) => self.parse_return_stmt(),
                Some(TokenKind::WhileKw) => self.parse_while_stmt(),
                Some(TokenKind::LoopKw) => self.parse_loop_stmt(),
                Some(TokenKind::BreakKw) => self.parse_break_stmt(),
                Some(TokenKind::ContinueKw) => self.parse_continue_stmt(),
                Some(TokenKind::Ident) if self.expr_followed_by_assignment() => {
                    self.parse_assign_stmt()
                }
                Some(_) if self.expr_starts() => {
                    if self.parse_expr_stmt_or_tail() {
                        break;
                    }
                }
                Some(TokenKind::Unknown) => {
                    self.error_here(DiagnosticKind::UnexpectedToken);
                    self.bump_as_error();
                }
                Some(_) => {
                    self.error_here(DiagnosticKind::ExpectedExpression);
                    self.bump_as_error();
                }
                None => break,
            }
            self.bump_trivia();
        }

        if self.at(TokenKind::RBrace) {
            self.bump();
        } else {
            self.push_diagnostic(
                kagari_common::Diagnostic::error(DiagnosticKind::ExpectedBlockEnd)
                    .with_span(self.peek().map(|token| token.span).unwrap_or_default()),
            );
        }

        self.finish_node();
    }

    fn parse_let_stmt(&mut self) {
        self.start_node(SyntaxKind::LetStmt);
        self.expect(TokenKind::LetKw, DiagnosticKind::ExpectedLetKeyword);
        self.parse_let_binding_name();
        self.bump_trivia();
        if self.at(TokenKind::Colon) {
            self.bump();
            self.parse_type_ref();
        }
        self.expect(TokenKind::Eq, DiagnosticKind::ExpectedLetInitializer);
        self.parse_expr();
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_return_stmt(&mut self) {
        self.start_node(SyntaxKind::ReturnStmt);
        self.expect(TokenKind::ReturnKw, DiagnosticKind::ExpectedReturnKeyword);
        self.bump_trivia();
        if !self.at(TokenKind::Semi) {
            self.parse_expr();
            self.bump_trivia();
        }
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_assign_stmt(&mut self) {
        self.start_node(SyntaxKind::AssignStmt);
        self.parse_place_expr();
        self.expect(TokenKind::Eq, DiagnosticKind::ExpectedLetInitializer);
        self.parse_expr();
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_while_stmt(&mut self) {
        self.start_node(SyntaxKind::WhileStmt);
        self.expect(TokenKind::WhileKw, DiagnosticKind::ExpectedWhileKeyword);
        self.parse_condition_expr();
        self.bump_trivia();
        self.parse_block();
        self.finish_node();
    }

    fn parse_loop_stmt(&mut self) {
        self.start_node(SyntaxKind::LoopStmt);
        self.expect(TokenKind::LoopKw, DiagnosticKind::ExpectedLoopKeyword);
        self.bump_trivia();
        self.parse_block();
        self.finish_node();
    }

    fn parse_break_stmt(&mut self) {
        self.start_node(SyntaxKind::BreakStmt);
        self.expect(TokenKind::BreakKw, DiagnosticKind::ExpectedBreakKeyword);
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_continue_stmt(&mut self) {
        self.start_node(SyntaxKind::ContinueStmt);
        self.expect(
            TokenKind::ContinueKw,
            DiagnosticKind::ExpectedContinueKeyword,
        );
        self.bump_trivia();
        self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        self.finish_node();
    }

    fn parse_expr_stmt_or_tail(&mut self) -> bool {
        let checkpoint = self.checkpoint();
        self.parse_expr();
        self.bump_trivia();

        if self.at(TokenKind::Semi) {
            self.start_node_at(checkpoint, SyntaxKind::ExprStmt);
            self.bump();
            self.finish_node();
            return false;
        }

        if self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            return true;
        }

        self.error_here(DiagnosticKind::ExpectedStatementTerminator);
        false
    }
}
