//! Statement/block construction and recursive assignment-place lowering.

use crate::{
    hir::{
        ids::{BlockId, PlaceId, StmtId},
        item::function::FunctionKind,
        pattern::{PatternData, PatternKind},
        place::{PlaceData, PlaceKind},
        stmt::{BlockData, StmtData, StmtKind},
        writeability::Writeability,
    },
    lower::context::{Lowerer, lower_binary_op, syntax_span, token_span},
};
use kagari_syntax::ast::{
    borrow::Ast,
    expr::BlockExpr,
    stmt::AssignmentOp,
    views::{ExprView, StmtView},
};

use smallvec::{SmallVec, smallvec};

impl Lowerer {
    /// Reserves a block, lowers statements in order, then lowers its optional tail.
    ///
    /// Cancellation stops the statement loop but still finalizes the partial block
    /// and restores the enclosing block. The analysis caller rejects cancelled work.
    pub(crate) fn lower_block(&mut self, block: &Ast<'_, BlockExpr>) -> BlockId {
        let id = self.alloc_block(
            syntax_span(block),
            BlockData {
                statements: SmallVec::new(),
                tail_expr: None,
            },
        );
        let previous_block = self.current_block.replace(id);
        let mut statements = SmallVec::<[_; 8]>::new();
        for stmt in block.statements() {
            if self.cancel.check().is_err() {
                break;
            }
            statements.push(self.lower_stmt(&stmt));
        }
        let tail_expr = block.tail_expr().map(|expr| self.lower_expr(&expr));

        self.module.body.blocks[id.index()].1 = BlockData {
            statements,
            tail_expr,
        };
        self.current_block = previous_block;
        id
    }

    /// Builds binding/control-flow records, using placeholders where required recovered syntax is absent.
    pub(crate) fn lower_stmt(&mut self, stmt: &StmtView<'_>) -> StmtId {
        let kind = match stmt {
            StmtView::FnDef(function) => StmtKind::Function(self.lower_function(
                function,
                FunctionKind::Local {
                    block: self.current_block.expect("statement block"),
                },
            )),
            StmtView::BindingStmt(stmt) => StmtKind::Binding {
                pattern: stmt
                    .pattern()
                    .map(|pattern| self.lower_pattern(&pattern))
                    .unwrap_or_else(|| self.synthetic_name_pattern("<missing>")),
                writeability: if stmt.is_var() {
                    Writeability::Var
                } else {
                    Writeability::Val
                },
                ty: stmt.ty().map(|ty| self.lower_type(&ty)),
                initializer: stmt
                    .initializer()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            StmtView::AssignStmt(stmt) => StmtKind::Assign {
                op: match stmt.operator() {
                    Some(AssignmentOp::Assign) => None,
                    Some(AssignmentOp::Compound(operator)) => Some(lower_binary_op(operator)),
                    None => {
                        let missing = self.missing_expr();
                        return self.alloc_stmt(
                            syntax_span(stmt),
                            StmtData {
                                kind: StmtKind::Expr(missing),
                            },
                        );
                    }
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
            StmtView::ReturnStmt(stmt) => StmtKind::Return {
                expr: stmt.expr().map(|expr| self.lower_expr(&expr)),
            },
            StmtView::WhileStmt(stmt) => StmtKind::While {
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
            StmtView::LoopStmt(stmt) => StmtKind::Loop {
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
            StmtView::ForStmt(stmt) => StmtKind::For {
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
            StmtView::BreakStmt(stmt) => match stmt.expr() {
                Some(expr) => StmtKind::BreakValue(self.lower_expr(&expr)),
                None => StmtKind::Break,
            },
            StmtView::ContinueStmt(_) => StmtKind::Continue,
            StmtView::Error(_) => StmtKind::Expr(self.missing_expr()),
            StmtView::ExprStmt(stmt) => StmtKind::Expr(
                stmt.expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            ),
        };

        self.alloc_stmt(syntax_span(stmt), StmtData { kind })
    }

    /// Retains field/index chains as places; general roots become expression-backed places for later validation.
    fn lower_place(&mut self, expr: &ExprView<'_>) -> PlaceId {
        match expr {
            ExprView::PathExpr(path) => self.alloc_place(
                syntax_span(path),
                PlaceData {
                    kind: PlaceKind::Name(path.name_text().unwrap_or_default()),
                },
            ),
            ExprView::FieldExpr(field) => {
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
            ExprView::IndexExpr(index_expr) => {
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
            ExprView::ParenExpr(paren) => paren
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
