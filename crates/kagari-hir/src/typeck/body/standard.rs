use crate::{
    hir::{expr::ExprKind, ids::ExprId},
    language::semantics::ProtocolSemantics,
    native::NativeTypeKind,
    typeck::{BodyTypeEnv, body::BodyChecker, completion, table::CallTarget},
    types::TypeId,
};
use kagari_abi::{language::Protocol, standard::surface::StandardEnum};
use kagari_common::diagnostic::{Diagnostic, DiagnosticKind};

impl BodyChecker<'_> {
    /// A completed branch can supply the missing payload type of a sibling None.
    /// Only result expressions participate; unrelated incomplete locals remain errors.
    pub(super) fn refine_standard_tail(
        &mut self,
        site: ExprId,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) {
        let mut pending = vec![site];
        while let Some(site) = pending.pop() {
            if self.cancel.check().is_err() {
                return;
            }
            let Some(mut actual) = self.type_table.expr_type(site) else {
                continue;
            };
            if actual.conflicts_with(expected) {
                continue;
            }
            actual.recover_from(expected);
            if self.type_table.enum_constructor(site).is_some() && !actual.is_unresolved() {
                let span = self.lowered.source_map.expr_span(site);
                self.diagnostics.retain(|d| {
                    !(d.span == Some(span)
                        && matches!(d.kind, DiagnosticKind::CannotInferGenericArgument { .. }))
                });
            }
            match &self.lowered.module.expr(site).kind {
                ExprKind::Block(block) => {
                    pending.extend(self.lowered.module.block(*block).tail_expr)
                }
                ExprKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    pending.extend(self.lowered.module.block(*then_branch).tail_expr);
                    pending.extend(else_branch.iter().copied());
                }
                _ => {}
            }
            self.type_table.insert_expr(site, actual.clone());
            env.exprs.insert(site, actual);
        }
    }
}

impl BodyChecker<'_> {
    pub(super) fn infer_propagation(
        &mut self,
        site: ExprId,
        operand: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let source_error = self.inference_variable(site, 2048);
        let context = match &self.expected_return {
            TypeId::StandardEnum { kind, args }
                if matches!(kind, StandardEnum::Option | StandardEnum::Result)
                    && args.len() == NativeTypeKind::Enum(*kind).arity() =>
            {
                let mut args = args.clone();
                args[0] = expected.cloned().unwrap_or(TypeId::Unknown);
                if *kind == StandardEnum::Result {
                    args[1] = source_error;
                }
                Some(TypeId::StandardEnum { kind: *kind, args })
            }
            _ => None,
        };
        let ty = self.infer_expr_type_expected(operand, env, context.as_ref());
        if !completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            operand,
            self.cancel,
        )
        .unwrap_or(false)
        {
            return TypeId::Unknown;
        }
        let TypeId::StandardEnum { kind, args } = &ty else {
            if !ty.is_unresolved() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                        function_name: "operator ?".into(),
                        expected: "Option<T> or Result<T, E>".into(),
                        found: ty.display_name(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
            return TypeId::Error;
        };
        if args.len() != NativeTypeKind::Enum(*kind).arity()
            || !matches!(kind, StandardEnum::Option | StandardEnum::Result)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                    function_name: "operator ?".into(),
                    expected: "Option<T> or Result<T, E>".into(),
                    found: ty.display_name(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
            return TypeId::Error;
        }
        let mut residual_args = args.clone();
        residual_args[0] = TypeId::Unknown;
        let residual = TypeId::StandardEnum {
            kind: *kind,
            args: residual_args,
        };
        if self.expected_return == TypeId::Unknown && !self.closure_returns.is_empty() {
            self.expected_return = residual.clone();
        }
        let compatible = match self.expected_return.clone() {
            TypeId::StandardEnum {
                kind: target,
                args: target_args,
            } if target == *kind && target_args.len() == NativeTypeKind::Enum(*kind).arity() => {
                if *kind == StandardEnum::Result {
                    let source = args[1].clone();
                    let target = target_args[1].clone();
                    if self.solving {
                        self.propagation_defaults
                            .push((source.clone(), target.clone()));
                        self.propagation_defaults
                            .push((target.clone(), source.clone()));
                    }
                    let mut interface = Protocol::From.nominal();
                    interface.arguments.push(source.clone());
                    if source.is_unresolved() || target.is_unresolved() {
                        true
                    } else if self
                        .aggregates
                        .implementation_count_bounded(&interface, &target, 4096, 64, self.cancel)
                        .is_ok_and(|count| count == 1)
                        && let Some(contract) = self.aggregates.trait_(&interface.declaration)
                        && let [method] = contract.methods.as_slice()
                    {
                        self.type_table
                            .insert_protocol_receiver(site, target.clone());
                        self.type_table.insert_call(
                            site,
                            CallTarget::TraitMethod {
                                method: method.id.clone(),
                                interface,
                            },
                            None,
                        );
                        self.record_protocol_application(site, &target, target.clone());
                        true
                    } else {
                        self.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                                type_name: target.display_name(),
                                trait_name: format!("From<{}>", source.display_name()),
                            })
                            .with_span(self.lowered.source_map.expr_span(site)),
                        );
                        true
                    }
                } else {
                    true
                }
            }
            _ => false,
        };
        if !compatible {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                    function_name: self.function_name.into(),
                    expected: self.expected_return.display_name(),
                    found: residual.display_name(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        if let Some(returns) = self.closure_returns.last_mut() {
            // The early return carries the converted error, not the operand's error.
            let mut returned = self.expected_return.clone();
            if let TypeId::StandardEnum { args, .. } = &mut returned {
                args[0] = TypeId::Unknown;
            }
            returns.push(returned);
        }
        args[0].clone()
    }
}
