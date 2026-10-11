//! Type arithmetic, comparison, indexing and condition operations.
//! Intrinsic scalar rules and declared operator protocols feed checked type/call facts;
//! invalid operands produce diagnostics without inventing executable operations.

use crate::{
    builtin::surface,
    hir::{
        expr::{
            ExprKind,
            literal::LiteralKind,
            ops::{BinaryOp, PrefixOp},
        },
        ids::ExprId,
    },
    language::{semantics as traits, semantics::callable_signature},
    typeck::{
        BodyTypeEnv, body::BodyChecker, completion, constraints, table::CallTarget,
        ty::display_type_id,
    },
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{cancellation::Cancelled, identity};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{
    language as standard_traits, language::Protocol, scalar::BuiltinType,
    surface::StandardTypeConstraint,
};

impl BodyChecker<'_> {
    pub(super) fn infer_prefix_operator(
        &mut self,
        expr_id: ExprId,
        op: &PrefixOp,
        expr: &ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        if matches!(op, PrefixOp::Neg)
            && let ExprKind::Literal(literal) = &self.lowered.module.expr(*expr).kind
            && matches!(literal.kind, LiteralKind::Number | LiteralKind::Float)
        {
            let ty = self.infer_numeric_literal(expr_id, literal, expected, true);
            self.type_table.insert_expr(*expr, ty.clone());
            return ty;
        }
        let inner = self.infer_expr_type_expected(*expr, env, expected);
        let Ok(completes) = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            *expr,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        if completes {
            let protocol = match op {
                PrefixOp::Neg => Protocol::Neg,
                PrefixOp::Not => Protocol::Not,
            };
            if let Some(requested) = self.aggregates.language_trait(protocol)
                && let Some(result) = self.record_operator(expr_id, *expr, &inner, requested, env)
            {
                return result;
            }
        }
        match op {
            PrefixOp::Neg => {
                if completes
                    && constraints::known_type_violates_constraint(
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
        expected: Option<&TypeId>,
    ) -> TypeId {
        let arithmetic = match op {
            BinaryOp::Add => Some(Protocol::Add),
            BinaryOp::Sub => Some(Protocol::Sub),
            BinaryOp::Mul => Some(Protocol::Mul),
            BinaryOp::Div => Some(Protocol::Div),
            BinaryOp::Rem => Some(Protocol::Rem),
            BinaryOp::BitAnd => Some(Protocol::BitAnd),
            BinaryOp::BitOr => Some(Protocol::BitOr),
            BinaryOp::BitXor => Some(Protocol::BitXor),
            BinaryOp::Shl => Some(Protocol::Shl),
            BinaryOp::Shr => Some(Protocol::Shr),

            _ => None,
        };
        let numeric_context =
            expected.filter(|ty| arithmetic.is_some() && matches!(ty, TypeId::Builtin(_)));
        let lhs_ty = self.infer_expr_type_expected(*lhs, env, numeric_context);
        let Ok(lhs_completes) = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            *lhs,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        let lhs_ty = lhs_completes.then_some(lhs_ty);
        let rhs_context = if let Some(protocol) = arithmetic {
            lhs_ty.as_ref().and_then(|left| {
                if matches!(op, BinaryOp::Shl | BinaryOp::Shr)
                    && matches!(left, TypeId::Builtin(_) | TypeId::Inference(_))
                {
                    return None;
                }
                let inputs: Vec<_> = self
                    .trait_bounds_for(left, env)
                    .into_iter()
                    .filter(|bound| bound.declaration == standard_traits::identity(protocol))
                    .filter_map(|bound| bound.arguments.into_iter().next())
                    .collect();
                if inputs.is_empty() && matches!(left, TypeId::Inference(_) | TypeId::Builtin(_)) {
                    return (!matches!(op, BinaryOp::Shl | BinaryOp::Shr)).then(|| left.clone());
                }
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
        let Ok(rhs_completes) = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            *rhs,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        if let (Some(protocol), Some(left)) = (arithmetic, lhs_ty.as_ref())
            && rhs_completes
            && let Some(mut requested) = self.aggregates.language_trait(protocol)
        {
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
            && let Some(requested) = self.aggregates.language_trait(Protocol::PartialOrd)
            && self
                .record_operator(expr_id, *lhs, left, requested, env)
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
        let context = self
            .trait_bounds_for(&receiver_ty, env)
            .into_iter()
            .find(|t| t.declaration == standard_traits::identity(Protocol::Index))
            .and_then(|t| t.arguments.into_iter().next());
        let index_ty = self.infer_expr_type_expected(*index, env, context.as_ref());
        let Ok(completes) = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            *receiver,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        if completes {
            if let Some(mut requested) = self.aggregates.language_trait(Protocol::Index) {
                requested.arguments.push(index_ty.clone());
                if let Some(result) =
                    self.record_operator(expr_id, *receiver, &receiver_ty, requested, env)
                {
                    return result;
                }
            }
            self.checked_index_type(*index, &receiver_ty, &index_ty, expr_id)
                .unwrap_or(TypeId::Error)
        } else {
            TypeId::Unknown
        }
    }
}

impl<'a> BodyChecker<'a> {
    pub(super) fn callable_contract(
        &self,
        ty: &TypeId,
        env: &BodyTypeEnv,
    ) -> Option<(NominalType, TypeId)> {
        let callable = self.aggregates.language_trait(Protocol::Fn)?;
        let mut candidates = self
            .trait_bounds_for(ty, env)
            .into_iter()
            .filter_map(|interface| {
                if interface.declaration != callable.declaration {
                    return None;
                }
                let (mut interface, output) = self.select_operator(ty, interface, env)?;
                interface.associated_types.insert(
                    identity::associated_type_id(&interface.declaration, "Output"),
                    output,
                );
                let signature = callable_signature(&interface)?;
                Some((interface, signature))
            });
        let selected = candidates.next()?;
        if candidates.next().is_some() {
            None
        } else {
            Some(selected)
        }
    }

    pub(super) fn select_operator(
        &self,
        ty: &TypeId,
        requested: NominalType,
        env: &BodyTypeEnv,
    ) -> Option<(NominalType, TypeId)> {
        let interface = if traits::intrinsic_applies(
            &requested,
            ty,
            Some(self.aggregates),
            &env.generic_bounds,
        ) {
            let mut interface = requested;
            if let Some(kind) = Protocol::from_id(&interface.declaration)
                && kind.iteration()
                && let Some(outputs) =
                    traits::iteration_outputs(kind, ty, Some(self.aggregates), &env.generic_bounds)
            {
                interface.associated_types.extend(outputs);
            }
            if let Some(output) = traits::intrinsic_output(&interface, ty) {
                interface.associated_types.insert(
                    identity::associated_type_id(&interface.declaration, "Output"),
                    output,
                );
            }
            interface
        } else {
            self.trait_bounds_for(ty, env).into_iter().find(|bound| {
                bound.declaration == requested.declaration && bound.arguments == requested.arguments
            })?
        };
        let contract = self.aggregates.trait_(&interface.declaration)?;
        let method = contract.methods.first()?;
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(contract.id.clone(), ty.clone());
        let result = self.aggregates.normalize_type(
            &method
                .return_type
                .with_self(&contract.id, ty)
                .instantiate(&substitution)
                .with_associated_types(&interface),
        );
        Some((interface, result))
    }

    /// Operator syntax and explicit method calls retain the same trait identity.
    pub(super) fn record_operator(
        &mut self,
        site: ExprId,
        receiver: ExprId,
        ty: &TypeId,
        requested: NominalType,
        env: &BodyTypeEnv,
    ) -> Option<TypeId> {
        let (interface, result) = self.select_operator(ty, requested, env)?;
        let method = self
            .aggregates
            .trait_(&interface.declaration)?
            .methods
            .first()?
            .id
            .clone();
        self.type_table.insert_call(
            site,
            CallTarget::TraitMethod { method, interface },
            Some(receiver),
        );
        self.record_protocol_application(site, ty, result.clone());
        Some(result)
    }

    pub(super) fn infer_binary_type(
        &mut self,
        op: BinaryOp,
        rhs_expr: ExprId,
        lhs_ty: Option<TypeId>,
        rhs_ty: Option<TypeId>,
        env: &BodyTypeEnv,
    ) -> TypeId {
        let produces_operands = lhs_ty.is_some() && rhs_ty.is_some();
        // An absent operand has no value constraint. Recovery types still retain
        // their own expression diagnostics and known counterpart constraints.
        let lhs_ty = lhs_ty.unwrap_or(TypeId::Unknown);
        let rhs_ty = rhs_ty.unwrap_or(TypeId::Unknown);
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                if !self.matching_numeric_operands(&lhs_ty, &rhs_ty, env) {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "matching numeric",
                        &lhs_ty,
                        &rhs_ty,
                        rhs_expr,
                    );
                }
                if !produces_operands {
                    TypeId::Unknown
                } else if matches!(lhs_ty, TypeId::Unknown | TypeId::Error)
                    || matches!(rhs_ty, TypeId::Unknown | TypeId::Error)
                {
                    TypeId::Error
                } else {
                    lhs_ty
                }
            }
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                let integer = |ty: &TypeId| {
                    ty.is_unresolved()
                        || matches!(ty, TypeId::Builtin(b) if b.integer_layout().is_some())
                };
                if !integer(&lhs_ty)
                    || !integer(&rhs_ty)
                    || (!matches!(op, BinaryOp::Shl | BinaryOp::Shr)
                        && lhs_ty.conflicts_with(&rhs_ty))
                {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "integer operands or an applicable operator trait",
                        &lhs_ty,
                        &rhs_ty,
                        rhs_expr,
                    );
                }
                if produces_operands {
                    lhs_ty
                } else {
                    TypeId::Unknown
                }
            }
            BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => {
                if (lhs_ty.conflicts_with(&rhs_ty)
                    && !lhs_ty.can_weaken_to(&rhs_ty)
                    && !rhs_ty.can_weaken_to(&lhs_ty)
                    && self
                        .aggregates
                        .shared_storage_view(&lhs_ty, &rhs_ty, self.cancel)
                        .is_none())
                    || [&lhs_ty, &rhs_ty].into_iter().any(|ty| {
                        !self.aggregates.has_storage_view(ty, self.cancel)
                            && !matches!(
                                ty,
                                TypeId::Unknown
                                    | TypeId::Error
                                    | TypeId::Struct(_)
                                    | TypeId::Array(_)
                                    | TypeId::Map { .. }
                                    | TypeId::Set(_, _)
                            )
                    })
                {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "matching identity-bearing object",
                        &lhs_ty,
                        &rhs_ty,
                        rhs_expr,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if (lhs_ty.conflicts_with(&rhs_ty)
                    && !lhs_ty.can_weaken_to(&rhs_ty)
                    && !rhs_ty.can_weaken_to(&lhs_ty)
                    && self
                        .aggregates
                        .shared_storage_view(&lhs_ty, &rhs_ty, self.cancel)
                        .is_none())
                    || [&lhs_ty, &rhs_ty].into_iter().any(|ty| {
                        !matches!(ty, TypeId::Unknown | TypeId::Error)
                            && !traits::intrinsic_holds(
                                Protocol::PartialEq,
                                ty,
                                Some(self.aggregates),
                                &env.generic_bounds,
                            )
                    })
                {
                    self.emit_binary_operand_type_mismatch(
                        op, "matching", &lhs_ty, &rhs_ty, rhs_expr,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                let supports_ordering = |ty: &TypeId| {
                    matches!(ty, TypeId::Unknown | TypeId::Error)
                        || self
                            .aggregates
                            .language_trait(Protocol::PartialOrd)
                            .and_then(|requested| self.select_operator(ty, requested, env))
                            .is_some()
                        || constraints::type_satisfies_standard_constraint(
                            ty,
                            StandardTypeConstraint::OrderedNumber,
                            &env.generic_bounds,
                            Some(self.aggregates),
                        )
                };
                if lhs_ty.conflicts_with(&rhs_ty)
                    || !supports_ordering(&lhs_ty)
                    || !supports_ordering(&rhs_ty)
                {
                    self.emit_binary_operand_type_mismatch(
                        op,
                        "matching PartialOrd",
                        &lhs_ty,
                        &rhs_ty,
                        rhs_expr,
                    );
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
            BinaryOp::AndAnd | BinaryOp::OrOr => {
                if lhs_ty.conflicts_with(&TypeId::Builtin(BuiltinType::Bool))
                    || rhs_ty.conflicts_with(&TypeId::Builtin(BuiltinType::Bool))
                {
                    self.emit_binary_operand_type_mismatch(op, "bool", &lhs_ty, &rhs_ty, rhs_expr);
                }
                TypeId::Builtin(BuiltinType::Bool)
            }
        }
    }

    pub(super) fn matching_numeric_operands(
        &self,
        lhs: &TypeId,
        rhs: &TypeId,
        env: &BodyTypeEnv,
    ) -> bool {
        if matches!(lhs, TypeId::Unknown | TypeId::Error)
            || matches!(rhs, TypeId::Unknown | TypeId::Error)
        {
            return ![lhs, rhs].into_iter().any(|ty| {
                constraints::known_type_violates_constraint(
                    ty,
                    StandardTypeConstraint::OrderedNumber,
                    &env.generic_bounds,
                )
            });
        }
        surface::supports_arithmetic(lhs, rhs)
            || (lhs == rhs
                && matches!(lhs, TypeId::Generic(_))
                && constraints::type_satisfies_standard_constraint(
                    lhs,
                    StandardTypeConstraint::OrderedNumber,
                    &env.generic_bounds,
                    Some(self.aggregates),
                ))
    }

    pub(super) fn emit_binary_operand_type_mismatch(
        &mut self,
        op: BinaryOp,
        expected: &'static str,
        lhs: &TypeId,
        rhs: &TypeId,
        span_expr: ExprId,
    ) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::BinaryOperandTypeMismatch {
                operator: self.binary_operator_name(op),
                expected: expected.to_owned(),
                lhs: display_type_id(lhs),
                rhs: display_type_id(rhs),
            })
            .with_span(self.lowered.source_map.expr_span(span_expr)),
        );
    }

    pub(super) fn binary_operator_name(&self, op: BinaryOp) -> &'static str {
        match op {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",

            BinaryOp::Eq => "==",
            BinaryOp::NotEq => "!=",
            BinaryOp::IdentityEq => "===",
            BinaryOp::IdentityNotEq => "!==",
            BinaryOp::Lt => "<",
            BinaryOp::Gt => ">",
            BinaryOp::Le => "<=",
            BinaryOp::Ge => ">=",
            BinaryOp::AndAnd => "&&",
            BinaryOp::OrOr => "||",
        }
    }

    pub(super) fn check_condition_type(
        &mut self,
        expr_id: ExprId,
        context: &'static str,
        env: &mut BodyTypeEnv,
    ) -> Result<bool, Cancelled> {
        let ty = self.infer_expr_type(expr_id, env);
        let completes = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            expr_id,
            self.cancel,
        )?;
        if completes && ty.conflicts_with(&TypeId::Builtin(BuiltinType::Bool)) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ConditionTypeMismatch {
                    context,
                    found: display_type_id(&ty),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
        Ok(completes)
    }
}
