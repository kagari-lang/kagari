use crate::{
    builtin::{
        declarations::{ApiBoundSemantics, ApiTypeSemantics, Arguments},
        traits::{self, StandardTraitSemantics},
    },
    hir::{ExprId, ExprKind, PatternId, PatternKind},
    resolver::ResolvedName,
    typeck::{
        BodyTypeEnv, CallTarget, ConstraintTarget,
        body::{BodyChecker, standard_intrinsic_name},
        completion,
    },
    types::TypeId,
};
use kagari_abi::standard::{
    StandardIntrinsic,
    declarations::ApiType,
    surface::{self as standard_surface, StandardEnum, StandardFunctionSpec},
    traits::StandardTrait,
};
use kagari_common::{Diagnostic, DiagnosticKind};

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
            if self.type_table.standard_constructor(site).is_some() && !actual.is_unresolved() {
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
    pub(super) fn infer_standard_constructor(
        &mut self,
        site: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let (callee, args) = match &self.lowered.module.expr(site).kind {
            ExprKind::Name { .. } => (site, Vec::new()),
            ExprKind::Call { callee, args, .. } => (*callee, args.to_vec()),
            _ => return None,
        };
        let Some(ResolvedName::StandardVariant(variant)) = self.names.expr_resolution(callee)
        else {
            return None;
        };
        let kind = variant.kind();
        let explicit = match &self.lowered.module.expr(callee).kind {
            ExprKind::Name {
                explicit_type: Some(ty),
                ..
            } => Some(self.resolve_constructor_type(*ty, env)),
            _ => None,
        };
        // Return expressions already receive their return context explicitly.
        // Ambient closure context must not determine unrelated local values.
        let context = explicit
            .as_ref()
            .or(expected.filter(|ty| **ty != TypeId::Unknown));
        let mut types = match context {
            Some(TypeId::StandardEnum { kind: actual, args })
                if *actual == kind && args.len() == kind.spec().arity =>
            {
                args.clone()
            }
            _ => (0..kind.spec().arity)
                .map(|i| self.inference_variable(site, 64 + i))
                .collect(),
        };
        let arity = usize::from(variant.payload().is_some());
        if let Some(arguments) = self.explicit_arguments.get(&callee) {
            self.used_explicit_arguments.insert(callee);
            if arguments.len() != types.len() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                        type_name: format!(
                            "expected {} type arguments, found {}",
                            types.len(),
                            arguments.len()
                        ),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
            }
            for (ty, explicit) in types.iter_mut().zip(arguments) {
                *ty = explicit.clone();
            }
        }
        if site != callee && arity == 0 {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                    type_name: format!("{variant:?} (unit variant; omit parentheses)"),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        } else if args.len() != arity || (site == callee && arity != 0) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: format!("{variant:?}"),
                    expected: arity,
                    found: args.len(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        for (i, arg) in args.iter().enumerate() {
            let index = variant.payload().filter(|_| i == 0);
            let expected = index.map(|index| types[index].clone());
            let actual = self.infer_expr_with_coercion(*arg, env, expected.as_ref());
            if !completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                *arg,
                self.cancel,
            )
            .unwrap_or(false)
            {
                continue;
            }
            if let Some(index) = index {
                let _ = self.solver.constrain(&types[index], &actual, self.cancel);
                types[index] = self.solver.resolve(&types[index]);
                if types[index].conflicts_with(&actual) {
                    self.emit_arg_mismatch(
                        &format!("{variant:?}"),
                        "value",
                        &types[index],
                        &actual,
                        *arg,
                    );
                } else {
                    types[index].recover_from(&actual);
                }
            }
        }
        self.type_table.insert_standard_constructor(site, variant);
        if args.iter().any(|arg| {
            !completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                *arg,
                self.cancel,
            )
            .unwrap_or(false)
        }) {
            return Some(TypeId::Unknown);
        }
        for (i, ty) in types.iter().enumerate() {
            if !self.solving && ty.is_unresolved() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                        function_name: format!("{variant:?}"),
                        parameter: if i == 0 { "T" } else { "E" }.into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
        }
        if !self.solving {
            types = types.iter().map(TypeId::diagnose_unknowns).collect();
        }
        Some(TypeId::StandardEnum { kind, args: types })
    }

    pub(super) fn check_standard_pattern(
        &mut self,
        pattern: PatternId,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) -> bool {
        let (path, fields) = match &self.lowered.module.pattern(pattern).kind {
            PatternKind::EnumVariant { path, fields } => (path.as_str(), fields.clone()),
            PatternKind::Name { name, .. } => (name.as_str(), Vec::new()),
            _ => return false,
        };
        let Some(variant) = self.names.pattern_variants.get(&pattern).copied() else {
            return false;
        };
        let TypeId::StandardEnum { kind, args } = expected else {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: expected.display_name(),
                    found: path.into(),
                })
                .with_span(self.lowered.source_map.pattern_span(pattern)),
            );
            return true;
        };
        if *kind != variant.kind()
            || args.len() != kind.spec().arity
            || fields.len() != usize::from(variant.payload().is_some())
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: expected.display_name(),
                    found: path.into(),
                })
                .with_span(self.lowered.source_map.pattern_span(pattern)),
            );
            return true;
        }
        self.type_table.insert_standard_pattern(pattern, variant);
        if let Some(index) = variant.payload() {
            self.check_pattern(fields[0], &args[index], env);
        }
        true
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
                if matches!(
                    kind,
                    kagari_abi::standard::surface::StandardEnum::Option
                        | kagari_abi::standard::surface::StandardEnum::Result
                ) && args.len() == kind.spec().arity =>
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
        if args.len() != kind.spec().arity
            || !matches!(
                kind,
                kagari_abi::standard::surface::StandardEnum::Option
                    | kagari_abi::standard::surface::StandardEnum::Result
            )
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
            } if target == *kind && target_args.len() == kind.spec().arity => {
                if *kind == StandardEnum::Result {
                    let source = args[1].clone();
                    let target = target_args[1].clone();
                    if self.solving {
                        self.propagation_defaults
                            .push((source.clone(), target.clone()));
                        self.propagation_defaults
                            .push((target.clone(), source.clone()));
                    }
                    let mut interface = StandardTrait::From.nominal();
                    interface.arguments.push(source.clone());
                    if source.is_unresolved() || target.is_unresolved() {
                        true
                    } else if self.conversion_holds(&interface, &target, env) {
                        self.type_table.insert_protocol_receiver(site, target);
                        self.type_table.insert_call(
                            site,
                            CallTarget::TraitMethod {
                                method: StandardTrait::From.contract().methods[0].id.clone(),
                                interface,
                            },
                            None,
                        );
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

impl<'a> BodyChecker<'a> {
    pub(super) fn infer_standard_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        context: Option<&TypeId>,
    ) -> Option<TypeId> {
        if let Some((intrinsic, receiver, receiver_ty)) = self.standard_method(callee, env) {
            self.type_table.insert_call(
                call_expr,
                CallTarget::StandardIntrinsic(intrinsic),
                Some(receiver),
            );
            return Some(self.infer_standard_intrinsic_type(
                call_expr,
                intrinsic,
                Some(receiver_ty),
                args,
                env,
                context,
            ));
        }

        let intrinsic = self.standard_function(callee)?;
        self.type_table
            .insert_call(call_expr, CallTarget::StandardIntrinsic(intrinsic), None);
        Some(self.infer_standard_intrinsic_type(call_expr, intrinsic, None, args, env, context))
    }

    pub(super) fn check_standard_parameter(
        &mut self,
        spec: &StandardFunctionSpec,
        parameter: &ApiType,
        actual: &TypeId,
        env: &BodyTypeEnv,
        site: ExprId,
    ) {
        let mut arguments = spec
            .type_params
            .iter()
            .map(|name| (*name, TypeId::Unknown))
            .collect();
        parameter.infer(actual, &mut arguments);
        for constraint in spec.constraints {
            if let Some(actual) = arguments.get(constraint.param) {
                self.check_standard_constraint(actual, constraint.constraint, env, site);
            }
        }
    }

    pub(super) fn infer_standard_intrinsic_type(
        &mut self,
        call_expr: ExprId,
        intrinsic: StandardIntrinsic,
        receiver_ty: Option<TypeId>,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        context: Option<&TypeId>,
    ) -> TypeId {
        let ExprKind::Call { callee, .. } = self.lowered.module.expr(call_expr).kind else {
            unreachable!("standard call expression");
        };

        let Some(spec) = standard_surface::standard_function_by_intrinsic(intrinsic) else {
            return TypeId::Error;
        };
        let api = spec.api;
        let name = standard_intrinsic_name(intrinsic);
        let offset = usize::from(receiver_ty.is_some());
        self.check_builtin_arity(
            name,
            api.params.len().saturating_sub(offset),
            args.len(),
            callee,
        );
        let mut bindings: Arguments = spec
            .type_params
            .iter()
            .enumerate()
            .map(|(i, name)| (*name, self.inference_variable(call_expr, 256 + i)))
            .collect();
        if let Some(explicit) = self.explicit_arguments.get(&callee) {
            self.used_explicit_arguments.insert(callee);
            let mut receiver_bindings: Arguments = spec
                .type_params
                .iter()
                .map(|name| (*name, TypeId::Unknown))
                .collect();
            if let Some(receiver) = &receiver_ty {
                api.params[0].ty.infer(receiver, &mut receiver_bindings);
            }
            let parameters = spec
                .type_params
                .iter()
                .filter(|name| matches!(receiver_bindings[**name], TypeId::Unknown))
                .collect::<Vec<_>>();
            if explicit.is_empty() || explicit.len() != parameters.len() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                        type_name: format!(
                            "expected {} type arguments, found {}",
                            parameters.len(),
                            explicit.len()
                        ),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
            }
            for (name, ty) in parameters.into_iter().zip(explicit) {
                bindings.insert(*name, ty.clone());
            }
        }
        if let Some(context) = context {
            if let TypeId::Trait(interface) = context
                && let Some(storage) = traits::collection_storage(interface)
            {
                let _ = self.solver.constrain(
                    &api.result.instantiate(&bindings),
                    &storage,
                    self.cancel,
                );
            }
            let _ = self
                .solver
                .constrain(&api.result.instantiate(&bindings), context, self.cancel);
            api.result.infer(context, &mut bindings);
        }
        if let Some(receiver) = receiver_ty {
            self.check_standard_parameter(spec, &api.params[0].ty, &receiver, env, callee);
            let _ = self.solver.constrain(
                &api.params[0].ty.instantiate(&bindings),
                &receiver,
                self.cancel,
            );
            api.params[0].ty.infer(&receiver, &mut bindings);
            let expected = api.params[0].ty.instantiate(&bindings);
            if expected.conflicts_with(&receiver) && !receiver.can_weaken_to(&expected) {
                self.emit_arg_mismatch(name, api.params[0].name, &expected, &receiver, callee);
            }
        }
        for (index, argument) in args.iter().enumerate() {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            let parameter = api.params.get(index + offset);
            let expected = parameter.map(|p| p.ty.instantiate(&bindings));
            let actual = self.infer_expr_with_coercion(*argument, env, expected.as_ref());
            if !completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                *argument,
                self.cancel,
            )
            .unwrap_or(false)
            {
                continue;
            }
            if let Some(parameter) = parameter {
                self.check_standard_parameter(spec, &parameter.ty, &actual, env, *argument);
                parameter.ty.infer(&actual, &mut bindings);
                let expected = parameter.ty.instantiate(&bindings);
                if expected.conflicts_with(&actual) {
                    self.emit_arg_mismatch(name, parameter.name, &expected, &actual, *argument);
                }
            }
        }
        let declared_bounds = api
            .bounds
            .iter()
            .map(|(target, bounds)| {
                (
                    target.instantiate(&bindings),
                    bounds
                        .iter()
                        .map(|b| ConstraintTarget::Trait(b.nominal(&bindings)))
                        .collect(),
                )
            })
            .collect();
        self.check_generic_call_bounds(&[], &declared_bounds, &Default::default(), env, callee);
        for ty in bindings.values_mut() {
            *ty = self.solver.resolve(ty);
        }
        self.type_table.insert_type_arguments(
            call_expr,
            spec.type_params
                .iter()
                .map(|name| bindings[*name].clone())
                .collect(),
        );
        self.aggregates
            .normalize_type(&api.result.instantiate(&bindings))
    }
}
