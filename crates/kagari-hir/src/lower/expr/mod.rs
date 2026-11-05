//! Expression transformations and source-site recording; parentheses do not allocate HIR nodes.

use crate::{
    hir::{
        expr::{
            ClosureParam, Condition, ExprData, ExprKind, FieldInit, MatchArm,
            literal::{Literal, LiteralKind},
            ops::PrefixOp,
        },
        ids::ExprId,
        stmt::BlockData,
        ty::{TypeData, TypeKind},
    },
    lower::context::{Lowerer, lower_binary_op, syntax_span, token_span},
};
use kagari_syntax::ast::{
    borrow::Ast,
    expr::{BindingCondition, PrefixOp as AstPrefixOp},
    views::{ExprView, StringPartView},
};

use kagari_source::literal::decode_string_fragment_with_cancellation;

mod literal;
mod pattern;

use smallvec::{SmallVec, smallvec};

impl Lowerer {
    /// Prefers binding-condition syntax over the plain expression accessor and supplies missing children for recovery.
    pub(crate) fn lower_condition(
        &mut self,
        binding: Option<Ast<'_, BindingCondition>>,
        plain: Option<ExprView<'_>>,
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

    /// Lowers one expression recursively and records its navigation sites.
    ///
    /// Parentheses return the inner ID directly. Interpolation becomes string/format
    /// parts, constructor shorthand creates name expressions, and calls retain callee,
    /// arguments and explicit types separately. Cancellation produces a missing node;
    /// the enclosing analysis operation must reject cancelled publication.
    pub(crate) fn lower_expr(&mut self, expr: &ExprView<'_>) -> ExprId {
        if self.cancel.check().is_err() {
            return self.missing_expr();
        }
        let kind = match expr {
            ExprView::BlockExpr(block) => ExprKind::Block(self.lower_block(block)),
            ExprView::PathExpr(path) => ExprKind::Name {
                name: path.name_text().unwrap_or_default(),
                explicit_type: path
                    .qualified_type()
                    .map(|ty| self.lower_qualified_type(&ty))
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
            ExprView::InterpolatedString(string) => {
                let mut parts = SmallVec::new();
                for part in string.parts() {
                    match part {
                        StringPartView::Text(token) => {
                            let Ok(text) = decode_string_fragment_with_cancellation(
                                token.text(),
                                &self.cancel,
                            ) else {
                                return self.missing_expr();
                            };
                            let text =
                                text.expect("FormatText tokens have validated literal syntax");
                            parts.push(self.alloc_expr(
                                token.body_span(),
                                ExprData {
                                    kind: ExprKind::Literal(Literal {
                                        kind: LiteralKind::String,
                                        text,
                                    }),
                                },
                            ));
                        }
                        StringPartView::Hole(part) => {
                            let expr = part
                                .path()
                                .map(|path| self.lower_expr(&ExprView::PathExpr(path)))
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
                }
                ExprKind::InterpolatedString(parts)
            }
            ExprView::Error(_) => ExprKind::Missing,
            ExprView::Literal(literal) => self.lower_literal_expr(literal),
            ExprView::ParenExpr(paren) => {
                return paren
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr());
            }
            ExprView::AwaitExpr(node) => ExprKind::Await {
                expr: node
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ExprView::PropagateExpr(node) => ExprKind::Propagate {
                expr: node
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ExprView::CastExpr(cast) => ExprKind::Cast {
                expr: cast
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                target: cast
                    .ty()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            },
            ExprView::PrefixExpr(prefix) => ExprKind::Prefix {
                op: match prefix.operator() {
                    Some(AstPrefixOp::Negate) => PrefixOp::Neg,
                    Some(AstPrefixOp::Not) => PrefixOp::Not,
                    None => return self.missing_expr(),
                },
                expr: prefix
                    .expr()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ExprView::BinaryExpr(binary) => ExprKind::Binary {
                lhs: binary
                    .lhs()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                op: match binary.operator() {
                    Some(operator) => lower_binary_op(operator),
                    None => return self.missing_expr(),
                },
                rhs: binary
                    .rhs()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ExprView::RangeExpr(range) => ExprKind::Range {
                start: range.start().map(|expr| self.lower_expr(&expr)),
                end: range.end().map(|expr| self.lower_expr(&expr)),
                inclusive: range.inclusive(),
            },

            ExprView::CallExpr(call) => ExprKind::Call {
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
            ExprView::FieldExpr(field) => ExprKind::Field {
                receiver: field
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                name: field.name_text().unwrap_or_default(),
            },
            ExprView::IndexExpr(index) => ExprKind::Index {
                receiver: index
                    .receiver()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                index: index
                    .index()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ExprView::IfExpr(if_expr) => ExprKind::If {
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
            ExprView::StructExpr(struct_expr) => ExprKind::StructInit {
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
                    .collect::<SmallVec<[_; 4]>>(),
                base: struct_expr.field_list().update_base().map(|base| {
                    base.value()
                        .map(|expr| self.lower_expr(&expr))
                        .unwrap_or_else(|| self.missing_expr())
                }),
            },
            ExprView::MatchExpr(match_expr) => ExprKind::Match {
                scrutinee: match_expr
                    .scrutinee()
                    .map(|expr| self.lower_expr(&expr))
                    .unwrap_or_else(|| self.missing_expr()),
                arms: match_expr
                    .arms()
                    .arms()
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
                    .collect::<SmallVec<[_; 4]>>(),
            },
            ExprView::LoopExpr(loop_expr) => ExprKind::Loop {
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
            ExprView::AsyncBlockExpr(block) => ExprKind::AsyncBlock {
                body: block
                    .body()
                    .map(|body| self.lower_expr(&ExprView::BlockExpr(body)))
                    .unwrap_or_else(|| self.missing_expr()),
            },
            ExprView::ClosureExpr(closure) => ExprKind::Closure {
                is_async: closure.is_async(),
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
            ExprView::TupleExpr(tuple) => ExprKind::Tuple(
                tuple
                    .elements()
                    .map(|expr| self.lower_expr(&expr))
                    .collect::<SmallVec<[_; 4]>>(),
            ),
            ExprView::ArrayExpr(array) if array.is_repeat() => {
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
            ExprView::ArrayExpr(array) => ExprKind::Array {
                elements: array
                    .elements()
                    .map(|expr| self.lower_expr(&expr))
                    .collect::<SmallVec<[_; 4]>>(),
                element_type: None,
            },
        };

        let id = self.alloc_expr(syntax_span(expr), ExprData { kind });
        match expr {
            ExprView::FieldExpr(field) => {
                if let Some(name) = field.name() {
                    self.source_map.insert_expr_reference(id, token_span(&name));
                }
            }
            ExprView::PathExpr(path) => {
                let mut prefix = String::new();
                let mut sites = Vec::new();
                let segments = path
                    .path()
                    .map(|path| path.segments().collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .chain(path.name());
                for segment in segments {
                    if let Some(name) = segment.text() {
                        if !prefix.is_empty() {
                            prefix.push_str("::");
                        }
                        prefix.push_str(&name);
                        sites.push((prefix.clone(), token_span(&segment)));
                    }
                }
                self.source_map.insert_expr_path(id, sites);
                let name = path
                    .name()
                    .or_else(|| path.path()?.segments().last())
                    .or_else(|| path.qualified_type()?.member());
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
            ExprView::StructExpr(struct_expr) => {
                if let Some(name) = struct_expr
                    .path()
                    .and_then(|path| path.name().or_else(|| path.path()?.segments().last()))
                {
                    self.source_map.insert_expr_reference(id, token_span(&name));
                }
                let spans = struct_expr
                    .field_list()
                    .fields()
                    .map(|field| field.name().map(|name| token_span(&name)))
                    .collect();
                self.source_map.insert_struct_fields(id, spans);
            }
            _ => {}
        }
        id
    }
}
