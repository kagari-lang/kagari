use kagari_syntax::ast;
use smallvec::{SmallVec, smallvec};

use crate::hir::{BlockData, BlockId, PlaceData, PlaceId, PlaceKind, StmtData, StmtId, StmtKind};
use crate::lower::context::{Lowerer, syntax_span};

impl Lowerer {
    pub(crate) fn lower_block(&mut self, block: &ast::BlockExpr) -> BlockId {
        let statements = block
            .statements()
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
            ast::Stmt::LetStmt(stmt) => StmtKind::Let {
                local: self.source_map.push_local(
                    stmt.name()
                        .map(|name| syntax_span(&name))
                        .unwrap_or_else(|| syntax_span(stmt)),
                ),
                mutable: stmt.is_mut(),
                name: stmt.name_text().unwrap_or_default(),
                ty: stmt.ty().map(|ty| self.lower_type(&ty)),
                initializer: stmt
                    .initializer()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
            },
            ast::Stmt::AssignStmt(stmt) => StmtKind::Assign {
                target: stmt
                    .target()
                    .map(|expr| self.lower_place(&expr))
                    .unwrap_or_else(|| self.synthetic_name_place("<missing>")),
                value: stmt
                    .value()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
            },
            ast::Stmt::ReturnStmt(stmt) => StmtKind::Return {
                expr: stmt.expr().map(|expr| self.lower_expr(&expr)),
            },
            ast::Stmt::WhileStmt(stmt) => StmtKind::While {
                condition: stmt
                    .condition()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
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
            ast::Stmt::LoopStmt(stmt) => StmtKind::Loop {
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
            ast::Stmt::BreakStmt(_) => StmtKind::Break,
            ast::Stmt::ContinueStmt(_) => StmtKind::Continue,
            ast::Stmt::ExprStmt(stmt) => StmtKind::Expr(
                stmt.expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
            ),
        };

        self.alloc_stmt(syntax_span(stmt), StmtData { kind })
    }

    fn lower_place(&mut self, expr: &ast::Expr) -> PlaceId {
        match expr {
            ast::Expr::PathExpr(path) => self.alloc_place(
                syntax_span(path),
                PlaceData {
                    kind: PlaceKind::Name(path.name_text().unwrap_or_default()),
                },
            ),
            ast::Expr::FieldExpr(field) => {
                let base = field
                    .receiver()
                    .map(|expr| self.lower_place(&expr))
                    .unwrap_or_else(|| self.synthetic_name_place("<missing>"));
                self.alloc_place(
                    syntax_span(field),
                    PlaceData {
                        kind: PlaceKind::Field {
                            base,
                            name: field.name_text().unwrap_or_default(),
                        },
                    },
                )
            }
            ast::Expr::IndexExpr(index_expr) => {
                let base = index_expr
                    .receiver()
                    .map(|expr| self.lower_place(&expr))
                    .unwrap_or_else(|| self.synthetic_name_place("<missing>"));
                let index = index_expr
                    .index()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>"));
                self.alloc_place(
                    syntax_span(index_expr),
                    PlaceData {
                        kind: PlaceKind::Index { base, index },
                    },
                )
            }
            _ => self.synthetic_name_place("<missing>"),
        }
    }
}
