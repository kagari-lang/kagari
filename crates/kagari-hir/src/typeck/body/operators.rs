use super::*;

impl BodyChecker<'_> {
    pub(super) fn infer_prefix_operator(
        &mut self,
        expr_id: ExprId,
        op: &PrefixOp,
        expr: &ExprId,
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        // The magnitude of MIN is not a positive i32 expression on its own.
        if matches!(op, PrefixOp::Neg)
            && let ExprKind::Literal(literal) = &self.lowered.module.expr(*expr).kind
            && literal.kind == LiteralKind::Number
            && kagari_common::literal::parse_integer_literal(&literal.text).ok() == Some(2147483648)
        {
            let ty = TypeId::Builtin(BuiltinType::I32);
            self.type_table
                .insert_scalar(expr_id, crate::typeck::ScalarValue::I32(i32::MIN));
            self.type_table.insert_expr(*expr, ty.clone());
            self.type_table.insert_expr(expr_id, ty.clone());
            env.exprs.insert(expr_id, ty.clone());
            return ty;
        }
        let inner = self.infer_expr_type(*expr, env);
        let Ok(completes) = crate::typeck::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            *expr,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        if completes {
            let protocol = match op {
                PrefixOp::Neg => crate::builtin::traits::StandardTrait::Neg,
                PrefixOp::Not => crate::builtin::traits::StandardTrait::Not,
            };
            if let Some(result) =
                self.record_operator(expr_id, *expr, &inner, protocol.nominal(), env)
            {
                return result;
            }
        }
        match op {
            PrefixOp::Neg => {
                if completes
                    && crate::typeck::constraints::known_type_violates_constraint(
                        &inner,
                        StandardTypeConstraint::SignedNumber,
                        &env.generic_bounds,
                    )
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                            operator: "-",
                            expected: "numeric".to_owned(),
                            found: display_type_id(&inner),
                        })
                        .with_span(self.lowered.source_map.expr_span(*expr)),
                    );
                }
                if completes { inner } else { TypeId::Unknown }
            }
            PrefixOp::Not => {
                if completes && inner.conflicts_with(&TypeId::Builtin(BuiltinType::Bool)) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnaryOperandTypeMismatch {
                            operator: "!",
                            expected: "bool".to_owned(),
                            found: display_type_id(&inner),
                        })
                        .with_span(self.lowered.source_map.expr_span(*expr)),
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
        }
    }
    pub(super) fn infer_binary_operator(
        &mut self,
        expr_id: ExprId,
        lhs: &ExprId,
        op: &BinaryOp,
        rhs: &ExprId,
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let arithmetic = match op {
            BinaryOp::Add => Some(crate::builtin::traits::StandardTrait::Add),
            BinaryOp::Sub => Some(crate::builtin::traits::StandardTrait::Sub),
            BinaryOp::Mul => Some(crate::builtin::traits::StandardTrait::Mul),
            BinaryOp::Div => Some(crate::builtin::traits::StandardTrait::Div),
            BinaryOp::Rem => Some(crate::builtin::traits::StandardTrait::Rem),
            _ => None,
        };
        let lhs_ty = self.infer_expr_type(*lhs, env);
        let Ok(lhs_completes) = crate::typeck::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            *lhs,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        let lhs_ty = lhs_completes.then_some(lhs_ty);
        let rhs_context = if let Some(protocol) = arithmetic {
            lhs_ty.as_ref().and_then(|left| {
                let inputs: Vec<_> = self
                    .trait_bounds_for(left, env)
                    .into_iter()
                    .filter(|bound| bound.declaration == protocol.contract().id)
                    .filter_map(|bound| bound.arguments.into_iter().next())
                    .collect();
                let first = inputs.first()?;
                inputs
                    .iter()
                    .all(|input| input == first)
                    .then(|| first.clone())
            })
        } else {
            match op {
                BinaryOp::AndAnd | BinaryOp::OrOr => Some(TypeId::Builtin(BuiltinType::Bool)),
                _ => lhs_ty.clone(),
            }
        };
        let rhs_ty = self.infer_expr_type_expected(*rhs, env, rhs_context.as_ref());
        let Ok(rhs_completes) = crate::typeck::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            *rhs,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        if let (Some(protocol), Some(left)) = (arithmetic, lhs_ty.as_ref())
            && rhs_completes
        {
            let mut requested = protocol.nominal();
            requested.arguments.push(rhs_ty.clone());
            if let Some(result) = self.record_operator(expr_id, *lhs, left, requested, env) {
                return result;
            }
        }
        if matches!(
            op,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        ) && let Some(left) = lhs_ty.as_ref()
            && !matches!(left, TypeId::Unknown | TypeId::Error)
            && rhs_completes
            && !left.conflicts_with(&rhs_ty)
            && self
                .record_operator(
                    expr_id,
                    *lhs,
                    left,
                    crate::builtin::traits::StandardTrait::PartialOrd.nominal(),
                    env,
                )
                .is_some()
        {
            TypeId::Builtin(BuiltinType::Bool)
        } else {
            self.infer_binary_type(*op, *rhs, lhs_ty, rhs_completes.then_some(rhs_ty), env)
        }
    }
    pub(super) fn infer_index_operator(
        &mut self,
        expr_id: ExprId,
        receiver: &ExprId,
        index: &ExprId,
        env: &mut BodyTypeEnv,
    ) -> TypeId {
        let receiver_ty = self.infer_expr_type(*receiver, env);
        let index_ty = self.infer_expr_type(*index, env);
        let Ok(completes) = crate::typeck::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            *receiver,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        if completes {
            let mut requested = crate::builtin::traits::StandardTrait::Index.nominal();
            requested.arguments.push(index_ty.clone());
            if let Some(result) =
                self.record_operator(expr_id, *receiver, &receiver_ty, requested, env)
            {
                return result;
            }
            self.checked_index_type(*index, &receiver_ty, &index_ty, expr_id)
                .unwrap_or(TypeId::Error)
        } else {
            TypeId::Unknown
        }
    }
}
