use crate::hir::{Literal, LiteralKind, TypeData, TypeKind};
use ast::{Expr, Interpolation};
use kagari_common::Span;
mod literal;
mod pattern;

use kagari_syntax::{
    ast::{self, AstNode},
    kind::SyntaxKind,
};
use smallvec::{SmallVec, smallvec};

use crate::{
    hir::{
        BlockData, ClosureParam, Condition, ExprData, ExprId, ExprKind, FieldInit, MatchArm,
        PrefixOp,
    },
    lower::context::{Lowerer, lower_binary_op, syntax_span, token_span},
};

impl Lowerer {
    pub(crate) fn lower_condition(
        &mut self,
        binding: Option<ast::BindingCondition>,
        plain: Option<ast::Expr>,
    ) -> Condition {
        if let Some(binding) = binding {
            Condition::Binding {
                pattern: binding
                    .pattern()
                    .map(|pattern| self.lower_pattern(&pattern))
                    .unwrap_or_else(|| self.synthetic_name_pattern("<missing>")),
                initializer: binding
                    .initializer()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            }
        } else {
            Condition::Expr(
                plain
                    .as_ref()
                    .map(|expr| self.lower_expr(expr))
                    .unwrap_or_else(|| self.missing_expr()),
            )
        }
    }

    pub(crate) fn lower_expr(&mut self, expr: &ast::Expr) -> ExprId {
        if self.cancel.check().is_err() {
            return self.missing_expr();
        }
        let kind = match expr {
            Expr::BlockExpr(block) => ExprKind::Block(self.lower_block(block)),
            Expr::PathExpr(path) => ExprKind::Name {
                name: path.name_text().unwrap_or_default(),
                explicit_type: path
                    .qualified_type()
                    .map(|ty| self.lower_type(&ty))
                    .or_else(|| {
                        path.generic_args().map(|arguments| {
                            let args = arguments.args().map(|arg| self.lower_type(&arg)).collect();
                            let name = path.path().and_then(|path| path.text()).unwrap_or_default();
                            let mut span = syntax_span(&arguments);
                            if let Some(base) = path.path() {
                                span.start = token_span(&base).start;
                            }
                            let bindings = self.lower_associated_bindings(&arguments);
                            let id = self.alloc_type(
                                span,
                                TypeData {
                                    kind: TypeKind::Generic {
                                        name,
                                        args,
                                        bindings,
                                        callable_syntax: false,
                                        positional_after_binding: arguments
                                            .positional_after_binding(),
                                    },
                                },
                            );
                            if let Some(base) = path.path().and_then(|path| path.segments().last())
                            {
                                self.source_map.insert_type_name(id, token_span(&base));
                            }
                            id
                        })
                    }),
            },
            Expr::InterpolatedString(string) => {
                let mut parts = SmallVec::new();
                for element in string.syntax().children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::FormatText {
                            let text = token.text().replace("{{", "{").replace("}}", "}");
                            parts.push(self.alloc_expr(
                                Span::new(
                                    usize::from(token.text_range().start()),
                                    usize::from(token.text_range().end()),
                                ),
                                ExprData {
                                    kind: ExprKind::Literal(Literal {
                                        kind: LiteralKind::String,
                                        text: format!("\"{text}\""),
                                    }),
                                },
                            ));
                        }
                    } else if let Some(part) = element.into_node().and_then(Interpolation::cast) {
                        let expr = part
                            .expr()
                            .map(|expr| self.lower_expr(&expr))
                            .unwrap_or_else(|| self.missing_expr());
                        parts.push(self.alloc_expr(
                            syntax_span(&part),
                            ExprData {
                                kind: ExprKind::FormatPart {
                                    expr,
                                    debug: part.debug(),
                                },
                            },
                        ));
                    }
                }
                ExprKind::InterpolatedString(parts)
            }
            Expr::Literal(literal) => ExprKind::Literal(self.lower_literal(literal)),
            Expr::ParenExpr(paren) => {
                return paren
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr());
            }
            Expr::PropagateExpr(node) => ExprKind::Propagate {
                expr: node
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            Expr::CastExpr(cast) => ExprKind::Cast {
                expr: cast
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                target: cast
                    .ty()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            },
            Expr::PrefixExpr(prefix) => ExprKind::Prefix {
                op: match prefix.operator() {
                    Some(SyntaxKind::Minus) => PrefixOp::Neg,
                    _ => PrefixOp::Not,
                },
                expr: prefix
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            Expr::BinaryExpr(binary) => ExprKind::Binary {
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
            Expr::RangeExpr(range) => ExprKind::Range {
                start: range.start().map(|expr| self.lower_expr(&expr)),
                end: range.end().map(|expr| self.lower_expr(&expr)),
                inclusive: range.inclusive(),
            },

            Expr::CallExpr(call) => ExprKind::Call {
                type_args: call
                    .generic_args()
                    .map(|args| args.args().map(|ty| self.lower_type(&ty)).collect()),
                callee: call
                    .callee()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                args: call
                    .args()
                    .map(|arg| self.lower_expr(&arg))
                    .collect::<SmallVec<[_; 4]>>(),
            },
            Expr::FieldExpr(field) => ExprKind::Field {
                receiver: field
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                name: field.name_text().unwrap_or_default(),
            },
            Expr::IndexExpr(index) => ExprKind::Index {
                receiver: index
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                index: index
                    .index()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            Expr::IfExpr(if_expr) => ExprKind::If {
                condition: self.lower_condition(if_expr.binding_condition(), if_expr.condition()),
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
            Expr::StructExpr(struct_expr) => ExprKind::StructInit {
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
                    let bindings = self.lower_associated_bindings(&arguments);
                    let id = self.alloc_type(
                        span,
                        TypeData {
                            kind: TypeKind::Generic {
                                name,
                                args,
                                bindings,
                                callable_syntax: false,
                                positional_after_binding: arguments.positional_after_binding(),
                            },
                        },
                    );
                    if let Some(base) = struct_expr
                        .path()
                        .and_then(|path| path.path()?.segments().last())
                    {
                        self.source_map.insert_type_name(id, token_span(&base));
                    }
                    id
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
                                    .unwrap_or_else(|| {
                                        let Some(name) = field.name() else {
                                            return self.missing_expr();
                                        };
                                        let span = token_span(&name);
                                        let id = self.alloc_expr(
                                            span,
                                            ExprData {
                                                kind: ExprKind::Name {
                                                    name: field.name_text().unwrap_or_default(),
                                                    explicit_type: None,
                                                },
                                            },
                                        );
                                        self.source_map.insert_expr_reference(id, span);
                                        id
                                    }),
                            })
                            .collect::<SmallVec<[_; 4]>>()
                    })
                    .unwrap_or_default(),
            },
            Expr::MatchExpr(match_expr) => ExprKind::Match {
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
                                guard: arm.guard().map(|expr| self.lower_expr(&expr)),
                                expr: arm
                                    .expr()
                                    .map(|expr| self.lower_expr(&expr))
                                    .unwrap_or_else(|| self.missing_expr()),
                            })
                            .collect::<SmallVec<[_; 4]>>()
                    })
                    .unwrap_or_default(),
            },
            Expr::LoopExpr(loop_expr) => ExprKind::Loop {
                body: loop_expr
                    .body()
                    .map(|body| self.lower_block(&body))
                    .unwrap_or_else(|| {
                        self.alloc_block(
                            syntax_span(loop_expr),
                            BlockData {
                                statements: smallvec![],
                                tail_expr: None,
                            },
                        )
                    }),
            },
            Expr::ClosureExpr(closure) => ExprKind::Closure {
                params: closure
                    .params()
                    .map(|param| ClosureParam {
                        name: param
                            .name()
                            .and_then(|name| name.text())
                            .unwrap_or_default(),
                        local: self.alloc_local_id(syntax_span(&param)),
                        ty: param.ty().map(|ty| self.lower_type(&ty)),
                    })
                    .collect(),
                body: closure
                    .body()
                    .map(|body| self.lower_expr(&body))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            Expr::TupleExpr(tuple) => ExprKind::Tuple(
                tuple
                    .elements()
                    .map(|expr| self.lower_expr(&expr))
                    .collect::<SmallVec<[_; 4]>>(),
            ),
            Expr::ArrayExpr(array) if array.is_repeat() => {
                let mut elements = array.elements();
                let value = elements
                    .next()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr());
                let count = elements
                    .next()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr());
                ExprKind::ArrayRepeat { value, count }
            }
            Expr::ArrayExpr(array) => ExprKind::Array(
                array
                    .elements()
                    .map(|expr| self.lower_expr(&expr))
                    .collect::<SmallVec<[_; 4]>>(),
            ),
        };

        let id = self.alloc_expr(syntax_span(expr), ExprData { kind });
        match expr {
            Expr::FieldExpr(field) => {
                if let Some(name) = field.name() {
                    self.source_map.insert_expr_reference(id, token_span(&name));
                }
            }
            Expr::PathExpr(path) => {
                let name = path
                    .name()
                    .or_else(|| path.path()?.segments().last())
                    .or_else(|| path.qualified_type()?.qualified_type()?.member());
                if let Some(name) = name {
                    self.source_map.insert_expr_reference(id, token_span(&name));
                }
                if let Some(path_segments) = path.path() {
                    let mut previous = None;
                    let mut last = None;
                    for segment in path_segments.segments() {
                        previous = last;
                        last = Some(segment);
                    }
                    let owner = if path.name().is_some() {
                        last
                    } else {
                        previous
                    };
                    if let Some(owner) = owner {
                        self.source_map.insert_expr_owner(id, token_span(&owner));
                    }
                }
            }
            Expr::StructExpr(struct_expr) => {
                if let Some(name) = struct_expr
                    .path()
                    .and_then(|path| path.name().or_else(|| path.path()?.segments().last()))
                {
                    self.source_map.insert_expr_reference(id, token_span(&name));
                }
                let spans = struct_expr
                    .field_list()
                    .map(|list| {
                        list.fields()
                            .map(|field| field.name().map(|name| token_span(&name)))
                            .collect()
                    })
                    .unwrap_or_default();
                self.source_map.insert_struct_fields(id, spans);
            }
            _ => {}
        }
        id
    }
}
