//! Infer branch results before committing implicit interface conversions.

use crate::{
    aggregates::implementations::ImplementationSearchError,
    hir::{
        expr::{Condition, MatchArm},
        ids::{BlockId, ExprId},
    },
    typeck::{
        BodyTypeEnv,
        body::{BodyChecker, solving::inference_hint},
        completion,
        ty::display_type_id,
    },
    types::TypeId,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::scalar::BuiltinType;

impl BodyChecker<'_> {
    pub(super) fn common_branch_interface(
        &mut self,
        inputs: &[TypeId],
        env: &BodyTypeEnv,
        expr: ExprId,
    ) -> Option<TypeId> {
        match self.aggregates.common_interfaces(
            self.declarations.definitions(),
            inputs,
            &env.generic_bounds,
            self.cancel,
        ) {
            Ok(mut candidates) => match candidates.len() {
                0 => None,
                1 => Some(TypeId::Trait(candidates.remove(0))),
                _ => {
                    let mut candidates = candidates
                        .into_iter()
                        .map(|candidate| {
                            display_type_id(
                                self.declarations.definitions(),
                                &TypeId::Trait(candidate),
                            )
                        })
                        .collect::<Vec<_>>();
                    candidates.sort();
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AmbiguousCommonInterface { candidates })
                            .with_span(self.lowered.source_map.expr_span(expr)),
                    );
                    Some(TypeId::Error)
                }
            },
            Err(ImplementationSearchError::Cancelled) => None,
            Err(ImplementationSearchError::LimitExceeded) => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                        resource: "common interface search",
                        limit: 100_000,
                    })
                    .with_span(self.lowered.source_map.expr_span(expr)),
                );
                None
            }
        }
    }

    pub(super) fn infer_if_type(
        &mut self,
        condition: &Condition,
        then_branch: BlockId,
        else_branch: Option<ExprId>,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let mut then_env = env.clone();
        let condition_completes = match condition {
            Condition::Expr(expr) => {
                let Ok(completes) = self.check_condition_type(*expr, "if", env) else {
                    return TypeId::Unknown;
                };
                completes
            }
            Condition::Binding {
                pattern,
                initializer,
            } => {
                let ty = self.infer_expr_type(*initializer, env);
                self.check_pattern(*pattern, &ty, &mut then_env);
                let Ok(completes) = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    *initializer,
                    self.cancel,
                ) else {
                    return TypeId::Unknown;
                };
                completes
            }
        };
        let mut then_ty = self.infer_block_types_expected(then_branch, &mut then_env, expected);
        match else_branch {
            Some(else_expr) => {
                let Ok(then_completes) = completion::block_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    then_branch,
                    self.cancel,
                ) else {
                    return TypeId::Unknown;
                };
                let mut else_context = expected.cloned();
                if condition_completes && then_completes {
                    if let Some(context) = &mut else_context {
                        context.recover_from(&inference_hint(&then_ty));
                    } else {
                        else_context = Some(inference_hint(&then_ty));
                    }
                }
                let else_ty = self.infer_expr_with_coercion(else_expr, env, else_context.as_ref());
                let Ok(else_completes) = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    else_expr,
                    self.cancel,
                ) else {
                    return TypeId::Unknown;
                };
                if !condition_completes {
                    TypeId::Unknown
                } else {
                    if !then_completes {
                        then_ty = else_ty;
                    } else if else_completes
                        && (then_ty.can_weaken_to(&else_ty) || else_ty.can_weaken_to(&then_ty))
                    {
                        then_ty = then_ty.read_only_view().expect("collection branch join");
                    } else if else_completes && then_ty.conflicts_with(&else_ty) {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::IfBranchTypeMismatch {
                                expected: display_type_id(
                                    self.declarations.definitions(),
                                    &then_ty,
                                ),
                                found: display_type_id(self.declarations.definitions(), &else_ty),
                            })
                            .with_span(self.lowered.source_map.expr_span(else_expr)),
                        );
                    } else if else_completes {
                        // Unify unresolved constructor payloads without converting values.
                        let _ = self.solver.constrain(&then_ty, &else_ty, self.cancel);
                        then_ty.recover_from(&else_ty);
                        if !then_ty.is_unresolved()
                            && let Some(tail) = self.lowered.module.block(then_branch).tail_expr
                        {
                            self.refine_standard_tail(tail, &then_ty, &mut then_env);
                        }
                    }
                    then_ty
                }
            }
            None if condition_completes => TypeId::Builtin(BuiltinType::Unit),
            None => TypeId::Unknown,
        }
    }

    pub(super) fn infer_match_type(
        &mut self,
        scrutinee: ExprId,
        arms: &[MatchArm],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let scrutinee_ty = self.infer_expr_type(scrutinee, env);
        let Ok(scrutinee_completes) = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            scrutinee,
            self.cancel,
        ) else {
            return TypeId::Unknown;
        };
        let scrutinee_ty = if scrutinee_completes {
            scrutinee_ty
        } else {
            TypeId::Unknown
        };
        let mut reachable = scrutinee_completes;
        let mut hint: Option<TypeId> = None;
        let mut completing = Vec::new();
        for arm in arms {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            let sibling = hint.as_ref().filter(|_| reachable).map(inference_hint);
            let context = expected.or(sibling.as_ref());
            let (found, arm_env) =
                self.infer_match_arm_type(arm, &scrutinee_ty, env, context, expected.is_some());
            if !reachable {
                continue;
            }
            reachable = !self
                .names
                .pattern_is_irrefutable(&self.lowered.module, arm.pattern)
                || arm.guard.is_some();
            let Ok(completes) = completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                arm.expr,
                self.cancel,
            ) else {
                return TypeId::Unknown;
            };
            if !completes {
                continue;
            }
            // Contextual literals and unresolved constructors can use earlier facts.
            // Do not replace those facts with an interface chosen from a partial join.
            if let Some(hint) = &mut hint {
                hint.recover_from(&found);
            } else {
                hint = Some(found.clone());
            }
            completing.push((arm.expr, found, arm_env));
        }
        let Some(mut result) = hint else {
            return TypeId::Builtin(BuiltinType::Unit);
        };
        let inputs = completing
            .iter()
            .map(|(_, ty, _)| ty.clone())
            .collect::<Vec<_>>();
        let host_view = result.read_only_view().filter(|target| {
            inputs.iter().any(|input| input != &result)
                && inputs
                    .iter()
                    .all(|input| input == target || input.can_weaken_to(target))
        });
        if let Some(view) = host_view {
            result = view;
        }
        for (expr, found, mut arm_env) in completing {
            if found.conflicts_with(&result) && !found.can_weaken_to(&result) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::MatchArmTypeMismatch {
                        expected: display_type_id(self.declarations.definitions(), &result),
                        found: display_type_id(self.declarations.definitions(), &found),
                    })
                    .with_span(self.lowered.source_map.expr_span(expr)),
                );
            } else {
                let _ = self.solver.constrain(&found, &result, self.cancel);
                if !result.is_unresolved() {
                    self.refine_standard_tail(expr, &result, &mut arm_env);
                }
            }
        }
        result
    }
}
