use crate::{
    hir::{
        expr::{Condition, ops::BinaryOp},
        ids::StmtId,
        stmt::StmtKind,
    },
    typeck::{
        BodyTypeEnv, applications,
        body::{BodyChecker, LoopResult},
        check, completion,
        ty::{TypeContext, display_type, display_type_id, resolve_type_in},
    },
    types::TypeId,
};
use kagari_common::{
    diagnostic::{Diagnostic, DiagnosticKind},
    span::Span,
};
use kagari_contract::scalar::BuiltinType;

impl<'a> BodyChecker<'a> {
    pub(super) fn check_stmt(&mut self, stmt_id: StmtId, env: &mut BodyTypeEnv) {
        if self.cancel.check().is_err() {
            return;
        }
        let stmt = self.lowered.module.stmt(stmt_id);
        match &stmt.kind {
            StmtKind::Binding {
                local,
                writeability,
                ty,
                initializer,
                ..
            } => {
                let annotation = ty.map(|ty| {
                    self.prepare_annotation_holes(ty);
                    let resolved = resolve_type_in(
                        &self.lowered.module,
                        ty,
                        TypeContext {
                            declarations: self.declarations,
                            generics: &env.generics,
                            self_type: None,
                            implementation: None,
                        },
                        self.type_table,
                        self.cancel,
                    );
                    if resolved.contains_projection() {
                        applications::validate(
                            &resolved,
                            &env.generic_bounds,
                            (self.aggregates, &self.names.hosts),
                            self.type_table,
                            self.lowered.source_map.type_span(ty),
                            self.diagnostics,
                            self.cancel,
                        );
                    }
                    let resolved = self.aggregates.normalize_type(&resolved);
                    if resolved.is_unresolved() {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                                type_name: display_type(&self.lowered.module, ty),
                            })
                            .with_span(self.lowered.source_map.type_span(ty)),
                        );
                    }
                    check::validate_standard_type_constraints(
                        &resolved,
                        &env.generic_bounds,
                        self.lowered.source_map.type_span(ty),
                        self.diagnostics,
                        self.cancel,
                        Some(self.aggregates),
                    );
                    resolved
                });
                let initializer_ty =
                    self.infer_expr_with_coercion(*initializer, env, annotation.as_ref());
                let local_ty = annotation.unwrap_or_else(|| initializer_ty.clone());
                applications::validate_imported_interface_type(
                    &local_ty,
                    self.aggregates,
                    self.lowered.source.module_identity(),
                    self.lowered.source_map.stmt_span(stmt_id),
                    self.diagnostics,
                    self.cancel,
                );
                applications::validate(
                    &local_ty,
                    &env.generic_bounds,
                    (self.aggregates, &self.declarations.hosts),
                    self.type_table,
                    self.lowered.source_map.stmt_span(stmt_id),
                    self.diagnostics,
                    self.cancel,
                );
                let Ok(completes) = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    *initializer,
                    self.cancel,
                ) else {
                    return;
                };
                if completes && local_ty.conflicts_with(&initializer_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                            expected: display_type_id(&local_ty),
                            found: display_type_id(&initializer_ty),
                        })
                        .with_span(self.lowered.source_map.expr_span(*initializer)),
                    );
                }
                env.locals.insert(*local, local_ty.clone());
                env.local_writeability.insert(*local, *writeability);
                self.type_table.insert_local(*local, local_ty);
            }
            StmtKind::Assign { target, value, op } => {
                let target_ty = self.resolve_assignment_target_type(*target, env);
                let target_completes = completion::place_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    *target,
                    self.cancel,
                )
                .unwrap_or(false);
                // Write permission does not erase the known target type needed by
                // contextual inference and tooling after an invalid assignment.
                let shifting = matches!(op, Some(BinaryOp::Shl | BinaryOp::Shr));
                let expected_ty = (!shifting)
                    .then(|| self.type_table.place_type(*target))
                    .flatten()
                    .filter(|_| target_completes);
                let value_ty = self.infer_expr_with_coercion(*value, env, expected_ty.as_ref());
                let Ok(completes) = completion::expr_can_complete(
                    &self.lowered.module,
                    self.names,
                    self.type_table,
                    *value,
                    self.cancel,
                ) else {
                    return;
                };
                let completes = completes && target_completes;
                if completes && let (Some(op), Some(expected)) = (op, &target_ty) {
                    self.infer_binary_type(
                        *op,
                        *value,
                        Some(expected.clone()),
                        Some(value_ty.clone()),
                        env,
                    );
                }
                match target_ty {
                    Some(expected)
                        if completes && !shifting && expected.conflicts_with(&value_ty) =>
                    {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                                expected: display_type_id(&expected),
                                found: display_type_id(&value_ty),
                            })
                            .with_span(self.lowered.source_map.place_span(*target)),
                        )
                    }
                    None => {
                        let reason = self.assignment_target_error_reason(*target, env);
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidAssignmentTarget { reason })
                                .with_span(self.lowered.source_map.place_span(*target)),
                        );
                    }
                    _ => {}
                }
            }
            StmtKind::Return { expr } => {
                let expected = self.expected_return.clone();
                let found = expr.map_or(TypeId::Builtin(BuiltinType::Unit), |expr| {
                    self.infer_expr_with_coercion(expr, env, Some(&expected))
                });
                if let Some(expr) = expr {
                    let Ok(completes) = completion::expr_can_complete(
                        &self.lowered.module,
                        self.names,
                        self.type_table,
                        *expr,
                        self.cancel,
                    ) else {
                        return;
                    };
                    if !completes {
                        return;
                    }
                }
                if let Some(returns) = self.closure_returns.last_mut() {
                    returns.push(found.clone());
                }
                if found.conflicts_with(&self.expected_return) {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                            function_name: self.function_name.to_string(),
                            expected: display_type_id(&self.expected_return),
                            found: display_type_id(&found),
                        })
                        .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                }
            }
            StmtKind::While { condition, body } => {
                let mut body_env = env.clone();
                match condition {
                    Condition::Expr(expr) => {
                        if self.check_condition_type(*expr, "while", env).is_err() {
                            return;
                        }
                    }
                    Condition::Binding {
                        pattern,
                        initializer,
                    } => {
                        let ty = self.infer_expr_type(*initializer, env);
                        self.check_pattern(*pattern, &ty, &mut body_env);
                    }
                }
                self.loop_depth += 1;
                self.loop_results.push(LoopResult::Statement);
                let _ = self.infer_block_types(*body, &mut body_env);
                self.loop_results.pop();
                self.loop_depth -= 1;
            }
            StmtKind::Loop { body } => {
                self.loop_depth += 1;
                self.loop_results.push(LoopResult::Statement);
                let _ = self.infer_block_types(*body, env);
                self.loop_results.pop();
                self.loop_depth -= 1;
            }
            StmtKind::For {
                pattern,
                iterable,
                body,
            } => {
                let iterable_ty = self.infer_expr_type(*iterable, env);
                let Some(element_ty) = self.infer_iteration(*iterable, &iterable_ty, env) else {
                    if !iterable_ty.is_unresolved() {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidForIterable {
                                type_name: display_type_id(&iterable_ty),
                            })
                            .with_span(self.lowered.source_map.expr_span(*iterable)),
                        );
                    }
                    return;
                };
                let mut body_env = env.clone();
                self.check_pattern(*pattern, &element_ty, &mut body_env);
                if !self
                    .names
                    .pattern_is_irrefutable(&self.lowered.module, *pattern)
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                            expected: "irrefutable for binding".into(),
                            found: "refutable pattern".into(),
                        })
                        .with_span(self.lowered.source_map.pattern_span(*pattern)),
                    );
                }
                self.loop_depth += 1;
                self.loop_results.push(LoopResult::Statement);
                let _ = self.infer_block_types(*body, &mut body_env);
                self.loop_results.pop();
                self.loop_depth -= 1;
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakOutsideLoop)
                            .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                } else {
                    self.record_loop_break_type(
                        TypeId::Builtin(BuiltinType::Unit),
                        false,
                        self.lowered.source_map.stmt_span(stmt_id),
                    );
                }
            }
            StmtKind::BreakValue(value) => {
                let context = match self.loop_results.last() {
                    Some(LoopResult::Expression(value)) => {
                        value.expected.as_ref().or(value.found.as_ref()).cloned()
                    }
                    _ => None,
                };
                let ty = self.infer_expr_type_expected(*value, env, context.as_ref());
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakOutsideLoop)
                            .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                } else {
                    self.record_loop_break_type(
                        ty,
                        true,
                        self.lowered.source_map.stmt_span(stmt_id),
                    );
                }
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ContinueOutsideLoop)
                            .with_span(self.lowered.source_map.stmt_span(stmt_id)),
                    );
                }
            }
            StmtKind::Expr(expr) => {
                let _ = self.infer_expr_type(*expr, env);
            }
        }
    }

    pub(super) fn record_loop_break_type(&mut self, ty: TypeId, has_value: bool, span: Span) {
        match self.loop_results.last().cloned() {
            Some(LoopResult::Statement) => {
                if has_value {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakValueOutsideLoopExpression)
                            .with_span(span),
                    );
                }
            }
            Some(LoopResult::Expression(value)) => {
                if let Some(expected) = value.expected.as_ref().or(value.found.as_ref())
                    && ty.conflicts_with(expected)
                {
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::BreakValueTypeMismatch {
                            expected: display_type_id(expected),
                            found: display_type_id(&ty),
                        })
                        .with_span(span),
                    );
                }
                if let Some(LoopResult::Expression(value)) = self.loop_results.last_mut() {
                    if let Some(previous) = &mut value.found {
                        previous.recover_from(&ty);
                    } else {
                        value.found = Some(ty);
                    }
                }
            }
            None => {}
        }
    }
}
