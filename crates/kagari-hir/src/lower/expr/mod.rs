mod literal;
mod pattern;

use kagari_syntax::ast;
use kagari_syntax::kind::SyntaxKind;
use smallvec::{SmallVec, smallvec};

use crate::hir::{BlockData, ExprData, ExprId, ExprKind, FieldInit, MatchArm, PrefixOp};
use crate::lower::context::{Lowerer, lower_binary_op, syntax_span};

impl Lowerer {
    pub(crate) fn lower_expr(&mut self, expr: &ast::Expr) -> ExprId {
        let kind = match expr {
            ast::Expr::BlockExpr(block) => ExprKind::Block(self.lower_block(block)),
            ast::Expr::PathExpr(path) => ExprKind::Name(path.name_text().unwrap_or_default()),
            ast::Expr::Literal(literal) => ExprKind::Literal(self.lower_literal(literal)),
            ast::Expr::ParenExpr(paren) => {
                return paren
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>"));
            }
            ast::Expr::PrefixExpr(prefix) => ExprKind::Prefix {
                op: match prefix.operator() {
                    Some(SyntaxKind::Minus) => PrefixOp::Neg,
                    _ => PrefixOp::Not,
                },
                expr: prefix
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
            },
            ast::Expr::BinaryExpr(binary) => ExprKind::Binary {
                lhs: binary
                    .lhs()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                op: lower_binary_op(binary.operator()),
                rhs: binary
                    .rhs()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
            },
            ast::Expr::CallExpr(call) => ExprKind::Call {
                callee: call
                    .callee()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                args: call
                    .args()
                    .map(|arg| self.lower_expr(&arg))
                    .collect::<SmallVec<[_; 4]>>(),
            },
            ast::Expr::FieldExpr(field) => ExprKind::Field {
                receiver: field
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                name: field.name_text().unwrap_or_default(),
            },
            ast::Expr::IndexExpr(index) => ExprKind::Index {
                receiver: index
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                index: index
                    .index()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
            },
            ast::Expr::IfExpr(if_expr) => ExprKind::If {
                condition: if_expr
                    .condition()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                then_branch: match if_expr.then_branch() {
                    Some(block) => self.lower_block(&block),
                    None => self.alloc_block(
                        syntax_span(if_expr),
                        BlockData {
                            statements: smallvec![],
                            tail_expr: None,
                        },
                    ),
                },
                else_branch: if_expr.else_branch().map(|expr| self.lower_expr(&expr)),
            },
            ast::Expr::StructExpr(struct_expr) => ExprKind::StructInit {
                path: struct_expr
                    .path()
                    .and_then(|path| path.name_text())
                    .unwrap_or_default(),
                fields: struct_expr
                    .field_list()
                    .map(|field_list| {
                        field_list
                            .fields()
                            .map(|field| FieldInit {
                                name: field.name_text().unwrap_or_default(),
                                value: field
                                    .value()
                                    .map(|expr| self.lower_expr(&expr))
                                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                            })
                            .collect::<SmallVec<[_; 4]>>()
                    })
                    .unwrap_or_default(),
            },
            ast::Expr::MatchExpr(match_expr) => ExprKind::Match {
                scrutinee: match_expr
                    .scrutinee()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                arms: match_expr
                    .arms()
                    .map(|arms| {
                        arms.arms()
                            .map(|arm| MatchArm {
                                pattern: arm
                                    .pattern()
                                    .map(|pattern| self.lower_pattern(&pattern))
                                    .unwrap_or_else(|| self.synthetic_name_pattern("<missing>")),
                                expr: arm
                                    .expr()
                                    .map(|expr| self.lower_expr(&expr))
                                    .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
                            })
                            .collect::<SmallVec<[_; 4]>>()
                    })
                    .unwrap_or_default(),
            },
            ast::Expr::TupleExpr(tuple) => ExprKind::Tuple(
                tuple
                    .elements()
                    .map(|expr| self.lower_expr(&expr))
                    .collect::<SmallVec<[_; 4]>>(),
            ),
            ast::Expr::ArrayExpr(array) => ExprKind::Array(
                array
                    .elements()
                    .map(|expr| self.lower_expr(&expr))
                    .collect::<SmallVec<[_; 4]>>(),
            ),
        };

        self.alloc_expr(syntax_span(expr), ExprData { kind })
    }
}
