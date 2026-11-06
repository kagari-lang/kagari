//! Block ownership, tail classification and syntactically spelled assignments.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        expr::{BinaryOp, BlockExpr, Expr},
        misc::{BindingKeyword, Writeability},
        site::Required,
        stmt::{
            AssignStmt, AssignmentOp, BindingStmt, BreakStmt, ContinueStmt, ExprStmt, ForStmt,
            Initializer, LoopStmt, ReturnStmt, Stmt, WhileStmt,
        },
    },
    kind::SyntaxKind,
    parser::core::{Built, Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn parse_block(&mut self) -> Built<BlockExpr> {
        self.with_struct_literals(true, |parser| {
            parser
                .with_nesting(Self::parse_block_inner)
                .unwrap_or_else(|| Built::missing(parser.anchor()))
        })
    }

    fn parse_block_inner(&mut self) -> Built<BlockExpr> {
        if !self.at(TokenKind::LBrace) {
            self.error_here(DiagnosticKind::ExpectedFunctionBodyStart);
            return Built::missing(self.anchor());
        }
        let mark = self.begin();
        let open = Required::Present(self.take());
        let mut statements = Vec::new();
        let mut tail = None;
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) && self.continue_parsing() {
            self.position(1, statements.len());
            let before = self.cursor();
            let statement = match self.current_kind() {
                TokenKind::FnKw => Some(self.local_function()),
                TokenKind::AsyncKw if self.nth_nontrivia_kind(1) == Some(TokenKind::FnKw) => {
                    Some(self.local_function())
                }
                TokenKind::Hash | TokenKind::PubKw
                    if matches!(
                        self.attributed_item_kind(),
                        Some(TokenKind::FnKw | TokenKind::AsyncKw)
                    ) =>
                {
                    Some(self.local_function())
                }
                TokenKind::StaticKw | TokenKind::Hash | TokenKind::PubKw
                    if self.attributed_item_kind() == Some(TokenKind::StaticKw) =>
                {
                    self.error_here(DiagnosticKind::UnexpectedToken);
                    let rejected = self.parse_static();
                    Some(Built::present(
                        Stmt::Error(rejected.site),
                        self.completed_depth,
                    ))
                }
                TokenKind::ValKw | TokenKind::VarKw => Some(self.parse_binding_stmt()),
                TokenKind::ReturnKw => Some(self.parse_return()),
                TokenKind::WhileKw => Some(self.parse_while()),
                TokenKind::LoopKw => Some(self.parse_loop_stmt()),
                TokenKind::ForKw => Some(self.parse_for()),
                TokenKind::BreakKw => Some(self.parse_break()),
                TokenKind::ContinueKw => Some(self.parse_continue()),
                _ if self.expr_starts() => {
                    let expression = self.parse_expr();
                    if self.assignment_operator().is_some() {
                        Some(self.parse_assignment(expression))
                    } else if self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
                        tail = match expression.node {
                            Required::Present(expr) => Some(expr),
                            Required::Missing { .. } => None,
                        };
                        break;
                    } else {
                        let mark = self.begin_at(self.first(&expression, before), expression.depth);
                        let terminator = if self.at(TokenKind::Semi) {
                            Some(Required::Present(self.take()))
                        } else if matches!(
                            expression.node,
                            Required::Present(Expr::IfExpr(_) | Expr::BlockExpr(_))
                        ) {
                            None
                        } else {
                            Some(self.expect(
                                TokenKind::Semi,
                                DiagnosticKind::ExpectedStatementTerminator,
                            ))
                        };
                        let site = self.end(mark);
                        Some(Built::present(
                            Stmt::ExprStmt(Box::new(ExprStmt {
                                site,
                                expression: expression.node,
                                terminator,
                            })),
                            self.completed_depth,
                        ))
                    }
                }
                _ => {
                    self.error_here(if self.at(TokenKind::Unknown) {
                        DiagnosticKind::UnexpectedToken
                    } else {
                        DiagnosticKind::ExpectedExpression
                    });
                    self.bump_as_error();
                    None
                }
            };
            if let Some(statement) = statement
                && let Required::Present(statement) = statement.node
            {
                statements.push(statement);
            }
            if self.cursor() == before && !self.at(TokenKind::Eof) {
                self.bump_as_error();
            }
        }
        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(mark);
        statements.shrink_to_fit();
        Built::present(
            BlockExpr {
                site,
                open,
                statements,
                tail,
                close,
            },
            self.completed_depth,
        )
    }

    fn local_function(&mut self) -> Built<Stmt> {
        let function = self.parse_function();
        Built {
            depth: function.depth,
            node: match function.node {
                Required::Present(function) => Required::Present(Stmt::FnDef(Box::new(function))),
                Required::Missing { at } => Required::Missing { at },
            },
        }
    }

    pub(crate) fn parse_binding_kind(
        &mut self,
        diagnostic: DiagnosticKind,
    ) -> Required<BindingKeyword> {
        if self.at_any(&[TokenKind::ValKw, TokenKind::VarKw]) {
            let writeability = if self.at(TokenKind::ValKw) {
                Writeability::Val
            } else {
                Writeability::Var
            };
            Required::Present(BindingKeyword {
                writeability,
                token: self.take(),
            })
        } else {
            self.error_here(diagnostic);
            Required::Missing { at: self.anchor() }
        }
    }

    fn parse_binding_stmt(&mut self) -> Built<Stmt> {
        let mark = self.begin();
        let binding = self.parse_binding_kind(DiagnosticKind::ExpectedBindingKeyword);
        let pattern = self.parse_binding_pattern();
        if self.at(TokenKind::Pipe) {
            self.position(2, 0);
            self.error_here(DiagnosticKind::UnexpectedToken);
            while !self.at_any(&[
                TokenKind::Colon,
                TokenKind::Eq,
                TokenKind::Semi,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]) && self.continue_parsing()
            {
                self.bump_as_error();
            }
        }
        let annotation = self.parse_annotation();
        let initializer = self.eat(TokenKind::Eq).map(|equal| {
            let expression = self.parse_expr();
            Initializer {
                equal,
                expression: expression.node,
            }
        });
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        Built::present(
            Stmt::BindingStmt(Box::new(BindingStmt {
                site,
                binding,
                pattern: pattern.node,
                annotation,
                initializer,
                terminator,
            })),
            self.completed_depth,
        )
    }

    fn parse_return(&mut self) -> Built<Stmt> {
        let mark = self.begin();
        let keyword = self.take();
        let value = (!self.at(TokenKind::Semi)).then(|| self.parse_expr());
        let value = value.and_then(|value| match value.node {
            Required::Present(value) => Some(value),
            Required::Missing { .. } => None,
        });
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        Built::present(
            Stmt::ReturnStmt(Box::new(ReturnStmt {
                site,
                keyword,
                value,
                terminator,
            })),
            self.completed_depth,
        )
    }

    fn assignment_operator(&self) -> Option<(AssignmentOp, usize)> {
        let operation = match self.current_kind() {
            TokenKind::Eq => return Some((AssignmentOp::Assign, 1)),
            TokenKind::PlusEq => BinaryOp::Add,
            TokenKind::MinusEq => BinaryOp::Subtract,
            TokenKind::StarEq => BinaryOp::Multiply,
            TokenKind::SlashEq => BinaryOp::Divide,
            TokenKind::PercentEq => BinaryOp::Remainder,
            TokenKind::AmpEq => BinaryOp::BitAnd,
            TokenKind::PipeEq => BinaryOp::BitOr,
            TokenKind::CaretEq => BinaryOp::BitXor,
            _ => {
                return match self.joint_shift() {
                    Some(SyntaxKind::ShlEq) => {
                        Some((AssignmentOp::Compound(BinaryOp::ShiftLeft), 2))
                    }
                    Some(SyntaxKind::ShrEq) => {
                        Some((AssignmentOp::Compound(BinaryOp::ShiftRight), 2))
                    }
                    _ => None,
                };
            }
        };
        Some((AssignmentOp::Compound(operation), 1))
    }

    fn parse_assignment(&mut self, target: Built<Expr>) -> Built<Stmt> {
        let mark = self.begin_at(self.first(&target, self.cursor()), target.depth);
        let (operation, pieces) = self.assignment_operator().expect("recognized assignment");
        let operator = Required::Present(self.operator(operation, pieces));
        let value = self.parse_expr();
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        Built::present(
            Stmt::AssignStmt(Box::new(AssignStmt {
                site,
                target: target.node,
                operator,
                value: value.node,
                terminator,
            })),
            self.completed_depth,
        )
    }

    fn parse_while(&mut self) -> Built<Stmt> {
        let mark = self.begin();
        let keyword = self.take();
        let (condition, body) = self.parse_condition_body();
        let site = self.end(mark);
        Built::present(
            Stmt::WhileStmt(Box::new(WhileStmt {
                site,
                keyword,
                condition: condition.node,
                body: body.node,
            })),
            self.completed_depth,
        )
    }

    fn parse_loop_stmt(&mut self) -> Built<Stmt> {
        let mark = self.begin();
        let keyword = self.take();
        let body = self.parse_block();
        let site = self.end(mark);
        Built::present(
            Stmt::LoopStmt(Box::new(LoopStmt {
                site,
                keyword,
                body: body.node,
            })),
            self.completed_depth,
        )
    }

    fn parse_for(&mut self) -> Built<Stmt> {
        let mark = self.begin();
        let keyword = self.take();
        let pattern = self.parse_match_pattern();
        let in_keyword = self.expect(TokenKind::InKw, DiagnosticKind::UnexpectedToken);
        let iterable = self.parse_condition_expr();
        let body = self.parse_block();
        let site = self.end(mark);
        Built::present(
            Stmt::ForStmt(Box::new(ForStmt {
                site,
                keyword,
                pattern: pattern.node,
                in_keyword,
                iterable: iterable.node,
                body: body.node,
            })),
            self.completed_depth,
        )
    }

    fn parse_break(&mut self) -> Built<Stmt> {
        let mark = self.begin();
        let keyword = self.take();
        let value = (!self.at(TokenKind::Semi)).then(|| self.parse_expr());
        let value = value.and_then(|value| match value.node {
            Required::Present(value) => Some(value),
            Required::Missing { .. } => None,
        });
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        Built::present(
            Stmt::BreakStmt(Box::new(BreakStmt {
                site,
                keyword,
                value,
                terminator,
            })),
            self.completed_depth,
        )
    }

    fn parse_continue(&mut self) -> Built<Stmt> {
        let mark = self.begin();
        let keyword = self.take();
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        Built::present(
            Stmt::ContinueStmt(Box::new(ContinueStmt {
                site,
                keyword,
                terminator,
            })),
            self.completed_depth,
        )
    }
}
