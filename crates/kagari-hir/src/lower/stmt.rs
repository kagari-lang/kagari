use crate::hir::BinaryOp;
use crate::hir::PatternData;
use crate::hir::PatternKind;
use ast::Expr;
use ast::Stmt;
use kagari_syntax::ast;
use kagari_syntax::kind::SyntaxKind;
use smallvec::{SmallVec, smallvec};

use crate::hir::{
    BlockData, BlockId, PlaceData, PlaceId, PlaceKind, StmtData, StmtId, StmtKind, Writeability,
};
use crate::lower::context::{Lowerer, syntax_span, token_span};

impl Lowerer {
    pub(crate) fn lower_block(&mut self, block: &ast::BlockExpr) -> BlockId {
        let cancel = self.cancel.clone();
        let statements = block
            .statements()
            .take_while(|_| cancel.check().is_ok())
            .map(|stmt| self.lower_stmt(&stmt))
            .collect::<SmallVec<[_; 8]>>();
        let tail_expr = block.tail_expr().map(|expr| self.lower_expr(&expr));

        self.alloc_block(
            syntax_span(block),
            BlockData {
                statements,
                tail_expr,
            },
        )
    }

    pub(crate) fn lower_stmt(&mut self, stmt: &ast::Stmt) -> StmtId {
        let kind = match stmt {
            Stmt::BindingStmt(stmt) => StmtKind::Binding {
                local: self.source_map.push_local(
                    stmt.name()
                        .map(|name| token_span(&name))
                        .unwrap_or_else(|| syntax_span(stmt)),
                ),
                writeability: if stmt.is_var() {
                    Writeability::Var
                } else {
                    Writeability::Val
                },
                name: stmt.name_text().unwrap_or_default(),
                ty: stmt.ty().map(|ty| self.lower_type(&ty)),
                initializer: stmt
                    .initializer()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            Stmt::AssignStmt(stmt) => StmtKind::Assign {
                op: match stmt.operator() {
                    Some(SyntaxKind::PlusEq) => Some(BinaryOp::Add),
                    Some(SyntaxKind::MinusEq) => Some(BinaryOp::Sub),
                    Some(SyntaxKind::StarEq) => Some(BinaryOp::Mul),
                    Some(SyntaxKind::PercentEq) => Some(BinaryOp::Rem),
                    Some(SyntaxKind::SlashEq) => Some(BinaryOp::Div),
                    Some(SyntaxKind::AmpEq) => Some(BinaryOp::BitAnd),
                    Some(SyntaxKind::PipeEq) => Some(BinaryOp::BitOr),
                    Some(SyntaxKind::CaretEq) => Some(BinaryOp::BitXor),
                    Some(SyntaxKind::ShlEq) => Some(BinaryOp::Shl),
                    Some(SyntaxKind::ShrEq) => Some(BinaryOp::Shr),
                    _ => None,
                },
                target: stmt
                    .target()
                    .map(|expr| self.lower_place(&expr))
                    .unwrap_or_else(|| self.synthetic_name_place("<missing>")),
                value: stmt
                    .value()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            Stmt::ReturnStmt(stmt) => StmtKind::Return {
                expr: stmt.expr().map(|expr| self.lower_expr(&expr)),
            },
            Stmt::WhileStmt(stmt) => StmtKind::While {
                condition: self.lower_condition(stmt.binding_condition(), stmt.condition()),
                body: match stmt.body() {
                    Some(body) => self.lower_block(&body),
                    None => self.alloc_block(
                        syntax_span(stmt),
                        BlockData {
                            statements: smallvec![],
                            tail_expr: None,
                        },
                    ),
                },
            },
            Stmt::LoopStmt(stmt) => StmtKind::Loop {
                body: match stmt.body() {
                    Some(body) => self.lower_block(&body),
                    None => self.alloc_block(
                        syntax_span(stmt),
                        BlockData {
                            statements: smallvec![],
                            tail_expr: None,
                        },
                    ),
                },
            },
            Stmt::ForStmt(stmt) => StmtKind::For {
                pattern: stmt
                    .pattern()
                    .map(|pattern| self.lower_pattern(&pattern))
                    .unwrap_or_else(|| {
                        self.alloc_pattern(
                            syntax_span(stmt),
                            PatternData {
                                kind: PatternKind::Wildcard,
                            },
                        )
                    }),
                iterable: stmt
                    .iterable()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                body: match stmt.body() {
                    Some(body) => self.lower_block(&body),
                    None => self.alloc_block(
                        syntax_span(stmt),
                        BlockData {
                            statements: smallvec![],
                            tail_expr: None,
                        },
                    ),
                },
            },
            Stmt::BreakStmt(stmt) => match stmt.expr() {
                Some(expr) => StmtKind::BreakValue(self.lower_expr(&expr)),
                None => StmtKind::Break,
            },
            Stmt::ContinueStmt(_) => StmtKind::Continue,
            Stmt::ExprStmt(stmt) => StmtKind::Expr(
                stmt.expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            ),
        };

        self.alloc_stmt(syntax_span(stmt), StmtData { kind })
    }

    fn lower_place(&mut self, expr: &ast::Expr) -> PlaceId {
        match expr {
            Expr::PathExpr(path) => self.alloc_place(
                syntax_span(path),
                PlaceData {
                    kind: PlaceKind::Name(path.name_text().unwrap_or_default()),
                },
            ),
            Expr::FieldExpr(field) => {
                let base = field
                    .receiver()
                    .map(|expr| self.lower_place(&expr))
                    .unwrap_or_else(|| self.synthetic_name_place("<missing>"));
                let id = self.alloc_place(
                    syntax_span(field),
                    PlaceData {
                        kind: PlaceKind::Field {
                            base,
                            name: field.name_text().unwrap_or_default(),
                        },
                    },
                );
                if let Some(name) = field.name() {
                    self.source_map.insert_place_member(id, token_span(&name));
                }
                id
            }
            Expr::IndexExpr(index_expr) => {
                let base = index_expr
                    .receiver()
                    .map(|expr| self.lower_place(&expr))
                    .unwrap_or_else(|| self.synthetic_name_place("<missing>"));
                let index = index_expr
                    .index()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr());
                self.alloc_place(
                    syntax_span(index_expr),
                    PlaceData {
                        kind: PlaceKind::Index { base, index },
                    },
                )
            }
            Expr::ParenExpr(paren) => paren
                .expr()
                .map(|expr| self.lower_place(&expr))
                .unwrap_or_else(|| self.synthetic_name_place("<missing>")),
            _ => {
                let value = self.lower_expr(expr);
                self.alloc_place(
                    syntax_span(expr),
                    PlaceData {
                        kind: PlaceKind::Expr(value),
                    },
                )
            }
        }
    }
}
