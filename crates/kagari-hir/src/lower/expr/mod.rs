mod literal;
mod pattern;

use kagari_syntax::ast;
use kagari_syntax::kind::SyntaxKind;
use smallvec::{SmallVec, smallvec};

use crate::hir::{BlockData, ExprData, ExprId, ExprKind, FieldInit, MatchArm, PrefixOp};
use crate::lower::context::{Lowerer, lower_binary_op, syntax_span};

impl Lowerer {
    pub(crate) fn lower_expr(&mut self, expr: &ast::Expr) -> ExprId {
        if self.cancel.check().is_err() {
            return self.missing_expr();
        }
        let kind = match expr {
            ast::Expr::BlockExpr(block) => ExprKind::Block(self.lower_block(block)),
            ast::Expr::PathExpr(path) => ExprKind::Name {
                name: path.name_text().unwrap_or_default(),
                explicit_type: path.generic_args().map(|arguments| {
                    let args = arguments.args().map(|arg| self.lower_type(&arg)).collect();
                    let name = path.path().and_then(|path| path.text()).unwrap_or_default();
                    let mut span = syntax_span(&arguments);
                    if let Some(base) = path.path() {
                        span.start = syntax_span(&base).start;
                    }
                    self.alloc_type(
                        span,
                        crate::hir::TypeData {
                            kind: crate::hir::TypeKind::Generic { name, args },
                        },
                    )
                }),
            },
            ast::Expr::Literal(literal) => ExprKind::Literal(self.lower_literal(literal)),
            ast::Expr::ParenExpr(paren) => {
                return paren
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr());
            }
            ast::Expr::PrefixExpr(prefix) => ExprKind::Prefix {
                op: match prefix.operator() {
                    Some(SyntaxKind::Minus) => PrefixOp::Neg,
                    _ => PrefixOp::Not,
                },
                expr: prefix
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ast::Expr::BinaryExpr(binary) => ExprKind::Binary {
                lhs: binary
                    .lhs()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                op: lower_binary_op(binary.operator()),
                rhs: binary
                    .rhs()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ast::Expr::CallExpr(call) => ExprKind::Call {
                callee: call
                    .callee()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                args: call
                    .args()
                    .map(|arg| self.lower_expr(&arg))
                    .collect::<SmallVec<[_; 4]>>(),
            },
            ast::Expr::FieldExpr(field) => ExprKind::Field {
                receiver: field
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                name: field.name_text().unwrap_or_default(),
            },
            ast::Expr::IndexExpr(index) => ExprKind::Index {
                receiver: index
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                index: index
                    .index()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ast::Expr::IfExpr(if_expr) => ExprKind::If {
                condition: if_expr
                    .condition()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
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
                explicit_type: struct_expr.generic_args().map(|arguments| {
                    let args = arguments.args().map(|arg| self.lower_type(&arg)).collect();
                    let name = struct_expr
                        .path()
                        .and_then(|path| path.name_text())
                        .unwrap_or_default();
                    let mut span = syntax_span(&arguments);
                    if let Some(path) = struct_expr.path() {
                        span.start = syntax_span(&path).start;
                    }
                    self.alloc_type(
                        span,
                        crate::hir::TypeData {
                            kind: crate::hir::TypeKind::Generic { name, args },
                        },
                    )
                }),
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
                                    .unwrap_or_else(|| self.missing_expr()),
                            })
                            .collect::<SmallVec<[_; 4]>>()
                    })
                    .unwrap_or_default(),
            },
            ast::Expr::MatchExpr(match_expr) => ExprKind::Match {
                scrutinee: match_expr
                    .scrutinee()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
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
                                    .unwrap_or_else(|| self.missing_expr()),
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

        let id = self.alloc_expr(syntax_span(expr), ExprData { kind });
        match expr {
            ast::Expr::FieldExpr(field) => {
                if let Some(name) = field.name() {
                    self.source_map
                        .insert_expr_reference(id, syntax_span(&name));
                }
            }
            ast::Expr::PathExpr(path) => {
                let name = path.name().or_else(|| path.path()?.segments().last());
                if let Some(name) = name {
                    self.source_map
                        .insert_expr_reference(id, syntax_span(&name));
                }
            }
            _ => {}
        }
        id
    }
}
