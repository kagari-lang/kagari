use crate::{
    callable::AppliedCallSignature,
    hir::{expr::ExprKind, ids::ExprId},
    typeck::{
        BodyTypeEnv,
        body::BodyChecker,
        completion, inference,
        table::{CallTarget, ConstraintTarget, ResolvedCall, propagation::ResolvedPropagation},
    },
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::identity::{DefinitionPath, associated_type_id};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::language::{self, Protocol, binding};

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
        let output = self.inference_variable(site, 2048);
        let residual = self.inference_variable(site, 2049);
        if let Some(expected) = expected {
            let _ = self.solver.constrain(&output, expected, self.cancel);
        }
        let ty = self.infer_expr_type(operand, env);
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
        let Some(mut requested) = self.aggregates.language_trait(Protocol::Try) else {
            return TypeId::Error;
        };
        let output_id = associated_type_id(&requested.declaration, "Output");
        let residual_id = associated_type_id(&requested.declaration, "Residual");
        requested
            .associated_types
            .insert(output_id.clone(), output.clone());
        requested
            .associated_types
            .insert(residual_id.clone(), residual.clone());
        self.constrain_declared_bound(&ty, &requested, env);
        let ty = self.solver.resolve(&ty);
        let Some((interface, _)) = self.select_operator(
            &ty,
            self.aggregates.language_trait(Protocol::Try).unwrap(),
            env,
        ) else {
            if !ty.is_unresolved() {
                self.propagation_bound_error(site, &ty, "Try".into());
            }
            return self.solver.resolve(&output);
        };
        let projection_interface = interface.clone();
        let mut outputs = Vec::new();
        for (member, fallback) in [(output_id.clone(), output), (residual_id.clone(), residual)] {
            let value = interface
                .associated_types
                .get(&member)
                .cloned()
                .unwrap_or_else(|| {
                    self.aggregates.normalize_type(&TypeId::Projection {
                        receiver: Box::new(ty.clone()),
                        interface: Box::new(projection_interface.clone()),
                        member: member.clone(),
                        arguments: vec![],
                    })
                });
            let _ = self.solver.constrain(&fallback, &value, self.cancel);
            outputs.push(self.solver.resolve(&value));
        }
        let output = outputs[0].clone();
        let residual = outputs[1].clone();
        if self.expected_return == TypeId::Unknown && !self.closure_returns.is_empty() {
            self.expected_return = self.propagation_return_context(&ty, &output_id);
        }
        let target = self.expected_return.clone();
        let Some(mut requested) = self.aggregates.language_trait(Protocol::FromResidual) else {
            return TypeId::Error;
        };
        requested.arguments.push(residual.clone());
        self.constrain_declared_bound(&target, &requested, env);
        self.defer_residual_conversions(&target, &requested);
        let requested = match self.solver.resolve(&TypeId::Trait(requested)) {
            TypeId::Trait(interface) => interface,
            _ => unreachable!(),
        };
        let target = self.solver.resolve(&target);
        let Some((residual_interface, _)) = self.select_operator(&target, requested.clone(), env)
        else {
            if !residual.is_unresolved() && !target.is_unresolved() {
                self.propagation_bound_error(
                    site,
                    &target,
                    format!("FromResidual<{}>", residual.display_name()),
                );
            }
            return output;
        };
        let Some(branch) = self.propagation_call(&ty, &interface, "branch") else {
            return TypeId::Error;
        };
        let Some(from_residual) =
            self.propagation_call(&target, &residual_interface, "from_residual")
        else {
            return TypeId::Error;
        };
        let expected_branch = TypeId::Enum(NominalType {
            declaration: binding::control_flow_declaration(),
            arguments: vec![residual.clone(), output.clone()],
            associated_types: Default::default(),
        });
        if branch.signature.as_ref().is_none_or(|signature| {
            signature.params != [ty.clone()] || signature.return_type != expected_branch
        }) || from_residual.signature.as_ref().is_none_or(|signature| {
            signature.params != [residual.clone()] || signature.return_type != target
        }) {
            self.propagation_bound_error(site, &ty, "checked Try/FromResidual signatures".into());
            return TypeId::Error;
        }
        let Some(enumeration) = self
            .aggregates
            .enumeration(&binding::control_flow_declaration())
        else {
            return TypeId::Error;
        };
        let Some(break_variant) = enumeration
            .variants
            .iter()
            .find(|variant| variant.name == "Break")
        else {
            return TypeId::Error;
        };
        let Some(continue_variant) = enumeration
            .variants
            .iter()
            .find(|variant| variant.name == "Continue")
        else {
            return TypeId::Error;
        };
        if enumeration.generic_params.len() != 2
            || enumeration.variants.len() != 2
            || break_variant.payload != [TypeId::Generic(enumeration.generic_params[0].clone())]
            || continue_variant.payload != [TypeId::Generic(enumeration.generic_params[1].clone())]
        {
            self.propagation_bound_error(site, &ty, "checked ControlFlow bindings".into());
            return TypeId::Error;
        }
        self.type_table.insert_propagation(
            site,
            ResolvedPropagation {
                branch,
                from_residual,
                return_type: target.clone(),
                break_variant: break_variant.id.clone(),
                continue_variant: continue_variant.id.clone(),
            },
        );
        let returned = self.propagation_return_context(&target, &output_id);
        if let Some(returns) = self.closure_returns.last_mut() {
            returns.push(returned);
        }
        output
    }

    fn propagation_bound_error(&mut self, site: ExprId, ty: &TypeId, trait_name: String) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                type_name: ty.display_name(),
                trait_name,
            })
            .with_span(self.lowered.source_map.expr_span(site)),
        );
    }

    fn defer_residual_conversions(&mut self, receiver: &TypeId, interface: &NominalType) {
        if !self.solving {
            return;
        }
        let mut candidates = Vec::new();
        for implementation in self
            .aggregates
            .implementations()
            .filter(|implementation| implementation.trait_type.declaration == interface.declaration)
            .take(4096)
        {
            if self.cancel.check().is_err() {
                return;
            }
            let mut substitution = TypeSubstitution::default();
            for (pattern, actual) in [
                (&implementation.for_type, receiver),
                (
                    &TypeId::Trait(implementation.trait_type.clone()),
                    &TypeId::Trait(interface.clone()),
                ),
            ] {
                if inference::infer(
                    pattern,
                    actual,
                    &implementation.generic_params,
                    &mut substitution,
                    self.cancel,
                    Some(self.aggregates),
                )
                .is_err()
                {
                    return;
                }
            }
            if !implementation
                .for_type
                .instantiate(&substitution)
                .conflicts_with(receiver)
                && !TypeId::Trait(implementation.trait_type.instantiate(&substitution))
                    .conflicts_with(&TypeId::Trait(interface.clone()))
            {
                candidates.push((implementation, substitution));
            }
        }
        let [(implementation, substitution)] = candidates.as_slice() else {
            return;
        };
        for (target, constraints) in &implementation.bounds {
            for constraint in constraints {
                if let ConstraintTarget::Trait(bound) = constraint
                    && bound.declaration == language::identity(Protocol::From)
                    && let [input] = bound.arguments.as_slice()
                {
                    let source = input.instantiate(substitution);
                    let target = target.instantiate(substitution);
                    self.propagation_defaults
                        .push((source.clone(), target.clone()));
                    self.propagation_defaults.push((target, source));
                }
            }
        }
    }

    fn propagation_call(
        &self,
        receiver: &TypeId,
        interface: &NominalType,
        name: &str,
    ) -> Option<ResolvedCall> {
        let contract = self.aggregates.trait_(&interface.declaration)?;
        let method = contract.methods.iter().find(|method| method.name == name)?;
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(contract.id.clone(), receiver.clone());
        let resolve = |ty: &TypeId| {
            self.aggregates.normalize_type(
                &ty.instantiate(&substitution)
                    .with_associated_types(interface),
            )
        };
        Some(ResolvedCall {
            target: CallTarget::TraitMethod {
                method: method.id.clone(),
                interface: interface.clone(),
            },
            receiver: None,
            type_arguments: vec![],
            signature: Some(AppliedCallSignature {
                params: method
                    .params
                    .iter()
                    .map(|param| resolve(&param.ty))
                    .collect(),
                return_type: resolve(&method.return_type),
            }),
        })
    }

    fn propagation_return_context(&self, receiver: &TypeId, output: &DefinitionPath) -> TypeId {
        let candidates: Vec<_> = self
            .aggregates
            .implementations()
            .filter(|implementation| {
                implementation.trait_type.declaration == language::identity(Protocol::Try)
            })
            .take(4096)
            .filter_map(|implementation| {
                self.cancel.check().ok()?;
                let mut substitution = TypeSubstitution::default();
                inference::infer(
                    &implementation.for_type,
                    receiver,
                    &implementation.generic_params,
                    &mut substitution,
                    self.cancel,
                    Some(self.aggregates),
                )
                .ok()?;
                if implementation
                    .for_type
                    .instantiate(&substitution)
                    .conflicts_with(receiver)
                {
                    return None;
                }
                if let Some(TypeId::Generic(parameter)) =
                    implementation.trait_type.associated_types.get(output)
                {
                    substitution.insert(parameter.clone(), TypeId::Unknown);
                }
                Some(implementation.for_type.instantiate(&substitution))
            })
            .collect();
        if let [context] = candidates.as_slice() {
            context.clone()
        } else {
            receiver.clone()
        }
    }
}
