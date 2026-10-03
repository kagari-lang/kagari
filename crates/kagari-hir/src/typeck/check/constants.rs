use crate::{
    builtin::surface,
    hir::{
        expr::{
            ExprKind,
            ops::{BinaryOp, PrefixOp},
        },
        ids::{ConstId, ExprId},
    },
    lower::LoweredModule,
    resolver::resolved::{ResolvedName, ResolvedNames},
    typeck::{TopLevelTypeIndex, const_budget::ConstBudget, table::TypeTable, ty::display_type_id},
    types::TypeId,
};
use kagari_abi::scalar::BuiltinType;
use kagari_common::{
    cancellation::CancellationToken,
    diagnostic::{Diagnostic, DiagnosticKind},
};
use smallvec::SmallVec;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConstVisitState {
    Visiting,
    Done,
}

pub(super) fn validate_const_initializers(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    top_level_index: &TopLevelTypeIndex,
    type_table: &TypeTable,
    cancel: &CancellationToken,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    budget: &mut ConstBudget,
) {
    struct ConstValidator<'a> {
        lowered: &'a LoweredModule,
        names: &'a ResolvedNames,
        top_level_index: &'a TopLevelTypeIndex,
        type_table: &'a TypeTable,
        cancel: &'a CancellationToken,
        diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
        states: HashMap<ConstId, ConstVisitState>,
        budget: &'a mut ConstBudget,
    }

    impl ConstValidator<'_> {
        fn validate_const(&mut self, const_id: ConstId) {
            if self.budget.exhausted || self.cancel.check().is_err() {
                return;
            }
            match self.states.get(&const_id) {
                Some(ConstVisitState::Done) => return,
                Some(ConstVisitState::Visiting) => {
                    let const_item = self.lowered.module.constant(const_id);
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ConstCycle {
                            const_name: const_item.name.clone(),
                        })
                        .with_span(self.lowered.source_map.const_span(const_id)),
                    );
                    return;
                }
                None => {}
            }

            let initializer = self.lowered.module.constant(const_id).initializer;
            if !self.budget.enter(
                self.lowered.source_map.expr_span(initializer),
                self.diagnostics,
            ) {
                return;
            }
            self.validate_const_inner(const_id);
            self.budget.leave();
        }

        fn validate_const_inner(&mut self, const_id: ConstId) {
            self.states.insert(const_id, ConstVisitState::Visiting);
            let const_item = self.lowered.module.constant(const_id);
            if let Some(const_ty) = self.top_level_index.consts.get(&const_id)
                && !supports_const_type(const_ty)
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                        const_name: const_item.name.clone(),
                        reason: format!(
                            "const type `{}` is heap-backed; const supports value types only",
                            display_type_id(const_ty)
                        ),
                    })
                    .with_span(self.lowered.source_map.const_span(const_id)),
                );
                self.states.insert(const_id, ConstVisitState::Done);
                return;
            }

            // The root was charged before checking whether its type is const-safe.
            self.validate_const_expr_inner(const_item.id, const_item.initializer);
            if self.budget.exhausted || self.cancel.check().is_err() {
                return;
            }
            let const_item = self.lowered.module.constant(const_id);
            if let (Some(declared), Some(actual)) = (
                self.top_level_index.consts.get(&const_id),
                self.type_table.expr_type(const_item.initializer),
            ) && declared.conflicts_with(&actual)
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                        const_name: const_item.name.clone(),
                        reason: format!(
                            "expected `{}`, found `{}`",
                            display_type_id(declared),
                            display_type_id(&actual)
                        ),
                    })
                    .with_span(self.lowered.source_map.expr_span(const_item.initializer)),
                );
            }
            self.states.insert(const_id, ConstVisitState::Done);
        }

        fn validate_const_expr(&mut self, owner: ConstId, expr_id: ExprId) {
            if self.cancel.check().is_err()
                || !self
                    .budget
                    .enter(self.lowered.source_map.expr_span(expr_id), self.diagnostics)
            {
                return;
            }
            self.validate_const_expr_inner(owner, expr_id);
            self.budget.leave();
        }

        fn validate_const_expr_inner(&mut self, owner: ConstId, expr_id: ExprId) {
            if self.cancel.check().is_err() {
                return;
            }
            let expr = self.lowered.module.expr(expr_id);
            match &expr.kind {
                ExprKind::Literal(_) => {}
                ExprKind::Name { .. } => {
                    let Some(resolved) = self.names.expr_resolution(expr_id) else {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer must use literals or other consts",
                        );
                        return;
                    };

                    match resolved {
                        ResolvedName::Const(id) => self.validate_const(id),
                        _ => self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer must use literals or other consts",
                        ),
                    }
                }
                ExprKind::Cast { expr, .. } => {
                    self.validate_const_expr(owner, *expr);
                    if !matches!((self.type_table.expr_type(*expr), self.type_table.expr_type(expr_id)), (Some(TypeId::Builtin(a)), Some(TypeId::Builtin(b))) if a.can_cast_to(b))
                    {
                        self.emit_invalid_const(owner, expr_id, "unsupported constant cast");
                    }
                }
                ExprKind::Prefix { op, expr } => {
                    self.validate_const_expr(owner, *expr);

                    let Some(expr_ty) = self.type_table.expr_type(*expr) else {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer has unknown operand type",
                        );
                        return;
                    };

                    let supported = match op {
                        PrefixOp::Neg => surface::supports_unary_negation(&expr_ty),
                        PrefixOp::Not => {
                            expr_ty == TypeId::Builtin(BuiltinType::Bool)
                                || matches!(expr_ty, TypeId::Builtin(b) if b.integer_layout().is_some())
                        }
                    };
                    if !supported {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "unsupported unary const expression",
                        );
                    }
                }
                ExprKind::Binary { lhs, op, rhs } => {
                    self.validate_const_expr(owner, *lhs);
                    self.validate_const_expr(owner, *rhs);

                    let lhs_ty = self.type_table.expr_type(*lhs);
                    let rhs_ty = self.type_table.expr_type(*rhs);
                    if !supports_const_binary(op, lhs_ty.as_ref(), rhs_ty.as_ref()) {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "unsupported binary const expression",
                        );
                    }
                }
                ExprKind::Tuple(elements) | ExprKind::Array(elements) => {
                    for element in elements {
                        self.validate_const_expr(owner, *element);
                    }
                }
                ExprKind::StructInit { fields, .. } => {
                    for field in fields {
                        self.validate_const_expr(owner, field.value);
                    }
                }
                _ => self.emit_invalid_const(
                    owner,
                    expr_id,
                    "unsupported const initializer expression",
                ),
            }

            if let Some(resolved) = self.names.expr_resolution(expr_id)
                && let ResolvedName::Const(id) = resolved
                && !self.top_level_index.consts.contains_key(&id)
            {
                self.emit_invalid_const(
                    owner,
                    expr_id,
                    "const initializer references an unresolved const type",
                );
            }
        }

        fn emit_invalid_const(&mut self, owner: ConstId, expr_id: ExprId, reason: &'static str) {
            if self.budget.exhausted || self.cancel.check().is_err() {
                return;
            }
            let const_item = self.lowered.module.constant(owner);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                    const_name: const_item.name.clone(),
                    reason: reason.to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
    }

    fn supports_const_binary(op: &BinaryOp, lhs: Option<&TypeId>, rhs: Option<&TypeId>) -> bool {
        match (op, lhs, rhs) {
            (
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr,
                Some(TypeId::Builtin(left)),
                Some(TypeId::Builtin(right)),
            ) => {
                left.integer_layout().is_some()
                    && right.integer_layout().is_some()
                    && (matches!(op, BinaryOp::Shl | BinaryOp::Shr) || left == right)
            }
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem,
                Some(lhs),
                Some(rhs),
            ) => surface::supports_arithmetic(lhs, rhs),
            (BinaryOp::Eq | BinaryOp::NotEq, Some(lhs), Some(rhs)) => lhs == rhs,
            (BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge, Some(lhs), Some(rhs)) => {
                surface::supports_ordering(lhs, rhs)
            }
            (
                BinaryOp::AndAnd | BinaryOp::OrOr,
                Some(TypeId::Builtin(BuiltinType::Bool)),
                Some(TypeId::Builtin(BuiltinType::Bool)),
            ) => true,
            _ => false,
        }
    }

    fn supports_const_type(ty: &TypeId) -> bool {
        surface::supports_const_type(ty)
    }

    let mut validator = ConstValidator {
        lowered,
        names,
        top_level_index,
        type_table,
        cancel,
        diagnostics,
        states: HashMap::new(),
        budget,
    };
    for const_item in &lowered.module.consts {
        if validator.budget.exhausted || cancel.check().is_err() {
            break;
        }
        validator.validate_const(const_item.id);
    }
}
