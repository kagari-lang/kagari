use crate::{
    builtin::traits::{self, intrinsic_holds},
    callable::{AppliedCallSignature, CallableSignature},
    hir::{ExprId, ExprKind},
    imports::ImportedFunction,
    native::NativeBinding,
    resolver::ResolvedName,
    typeck::{
        BodyTypeEnv, CallTarget, ConstraintTarget, FunctionImplementation, GenericBounds,
        ScalarValue,
        body::BodyChecker,
        check, completion, constraints, inference, members,
        ty::{TypeContext, display_type_id, resolve_named_type},
    },
    types::{GenericParameterType, TypeId, TypeSubstitution},
};
use kagari_abi::standard::{
    surface::{self as standard_surface, StandardTypeConstraint},
    traits::StandardTrait,
};
use kagari_common::{Diagnostic, DiagnosticKind};
use std::iter;

impl<'a> BodyChecker<'a> {
    pub(super) fn infer_function_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        if let Some(imported) = self
            .names
            .expr_resolution(callee)
            .and_then(|name| self.imported_functions.get(name))
            .or_else(|| self.associated_function(callee))
        {
            let local = imported.id.file == self.lowered.source.id()
                && imported.id.revision == self.lowered.source.revision();
            self.type_table.insert_call(
                call_expr,
                if local {
                    CallTarget::Function(imported.id.function)
                } else {
                    CallTarget::SourceFunction(imported.declaration.clone())
                },
                None,
            );
            if !local
                && !imported.signature.generic_params.is_empty()
                && !matches!(
                    imported.signature.implementation,
                    FunctionImplementation::Native(NativeBinding::Provider(_))
                )
            {
                self.infer_typed_args(
                    args,
                    imported
                        .signature
                        .params
                        .iter()
                        .map(|parameter| self.aggregates.normalize_type(&parameter.ty))
                        .collect::<Vec<_>>()
                        .into_iter(),
                    env,
                );
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::PublicGenericFunction {
                        name: imported.signature.name.clone(),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
                return TypeId::Error;
            }
            return self.infer_checked_function_call(
                &imported.signature,
                call_expr,
                callee,
                args,
                env,
                expected,
            );
        }
        let Some(ResolvedName::Function(id)) = self.names.expr_resolution(callee) else {
            let callee_ty = self.infer_expr_type(callee, env);
            if let TypeId::Function { params, result } = &callee_ty {
                let arguments = self.infer_typed_args(args, params.iter().cloned(), env);
                self.type_table
                    .insert_call(call_expr, CallTarget::Value, Some(callee));
                self.type_table.insert_call_signature(
                    call_expr,
                    AppliedCallSignature {
                        params: params.clone(),
                        return_type: result.as_ref().clone(),
                    },
                );
                self.check_builtin_arity("closure", params.len(), args.len(), callee);
                for (index, ty) in params.iter().enumerate() {
                    self.check_arg_type(
                        "closure",
                        &format!("arg{index}"),
                        ty.clone(),
                        index,
                        &arguments,
                    );
                }
                return result.as_ref().clone();
            }
            if let Some((interface, TypeId::Function { params, result })) =
                self.callable_contract(&callee_ty, env)
            {
                let arguments = self.infer_typed_args(args, params.iter().cloned(), env);
                self.record_operator(call_expr, callee, &callee_ty, interface, env);
                self.check_builtin_arity("callable", params.len(), args.len(), callee);
                for (index, ty) in params.iter().enumerate() {
                    self.check_arg_type(
                        "callable",
                        &format!("arg{index}"),
                        ty.clone(),
                        index,
                        &arguments,
                    );
                }
                return *result;
            }
            self.infer_call_args(args, env);
            let Ok(completes) = completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                callee,
                self.cancel,
            ) else {
                return TypeId::Unknown;
            };
            if !completes {
                self.type_table
                    .insert_call(call_expr, CallTarget::TerminatingCallee, Some(callee));
                return TypeId::Unknown;
            }
            if !callee_ty.is_unresolved() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                        type_name: display_type_id(&callee_ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
            }
            return TypeId::Error;
        };
        let Some(function) = self.function_index.by_id.get(&id) else {
            self.infer_call_args(args, env);
            return self.infer_expr_type(callee, env);
        };
        self.type_table
            .insert_call(call_expr, CallTarget::Function(id), None);
        self.infer_checked_function_call(function, call_expr, callee, args, env, expected)
    }

    pub(super) fn associated_owner_shadowed(&self, callee: ExprId) -> bool {
        self.names.qualified_member(callee).is_some_and(|member| {
            !matches!(
                member.owner,
                ResolvedName::OpaqueType(_)
                    | ResolvedName::Struct(_)
                    | ResolvedName::Enum(_)
                    | ResolvedName::Trait(_)
                    | ResolvedName::HostType(_)
            ) && self
                .declarations
                .imported_types()
                .resolved(member.owner)
                .is_none()
        })
    }

    /// Resolve associated functions from checked impl members when a primitive
    /// owner or local type has no imported callable name. Lexical bindings win.
    pub(super) fn associated_function(&self, callee: ExprId) -> Option<&'a ImportedFunction> {
        if self.names.expr_resolution(callee).is_some() || self.associated_owner_shadowed(callee) {
            return None;
        }
        let ExprKind::Name { name, .. } = &self.lowered.module.expr(callee).kind else {
            return None;
        };
        let (owner, member) = name.rsplit_once("::")?;
        let ty = resolve_named_type(
            owner,
            TypeContext {
                declarations: self.declarations,
                generics: &[],
                self_type: None,
                implementation: None,
            },
        )
        .ty;
        if ty.is_unresolved() {
            return None;
        }
        let mut methods = self.aggregates.inherent_methods().filter(|method| {
            members::inherent_substitution(self.aggregates, method, &ty, self.cancel).is_some()
                && method.function.name == member
                && method.visibility.allows(
                    &method.declaration.module,
                    self.lowered.source.module_identity(),
                )
        });
        let method = methods.next()?;
        if methods.next().is_some() {
            return None;
        }
        self.imported_functions.target(&method.declaration)
    }

    pub(super) fn infer_checked_function_call(
        &mut self,
        function: &impl CallableSignature,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let mut substitution = TypeSubstitution::default();
        let empty_bounds = GenericBounds::new();
        let bounds = function.bounds().unwrap_or(&empty_bounds);
        self.seed_callable_context(callee, function.generic_params(), bounds, &mut substitution);
        self.seed_explicit_arguments(callee, function.generic_params(), &mut substitution);
        if let Some(expected) = expected
            && inference::infer(
                function.return_type(),
                expected,
                function.generic_params(),
                &mut substitution,
                self.cancel,
            )
            .is_err()
        {
            return TypeId::Unknown;
        }
        let arg_tys = self.infer_bounded_args(
            args,
            function.parameters().map(|(_, ty)| ty.clone()),
            function.generic_params(),
            &mut substitution,
            bounds,
            env,
        );
        if self.cancel.check().is_err() {
            return TypeId::Unknown;
        }
        let mut suppress_missing = arg_tys.iter().any(|(_, ty)| ty.is_unresolved());
        for (argument, _) in &arg_tys {
            let Ok(completes) = completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                *argument,
                self.cancel,
            ) else {
                return TypeId::Unknown;
            };
            suppress_missing |= !completes;
            if !completes {
                // A terminating argument produces no callable to specialize.
                // Keep concrete constraints from other inputs, but discard fresh
                // placeholders that were introduced only for callback context.
                substitution
                    .retain(|_, ty| !matches!(self.solver.resolve(ty), TypeId::Inference(_)));
            }
        }
        let type_arguments = self.finish_inferred_arguments(
            &mut substitution,
            function.generic_params(),
            function.name(),
            callee,
            suppress_missing,
        );
        self.type_table
            .insert_type_arguments(call_expr, type_arguments);
        self.check_generic_call_bounds(
            function.generic_params(),
            bounds,
            &substitution,
            env,
            callee,
        );
        let implicit = usize::from(
            self.type_table
                .call_resolution(call_expr)
                .is_some_and(|call| call.receiver.is_some()),
        );
        self.check_function_arguments(function, &substitution, callee, &arg_tys, implicit);
        self.record_callable_application(call_expr, function, &substitution)
    }

    pub(super) fn record_callable_application(
        &mut self,
        site: ExprId,
        function: &impl CallableSignature,
        substitution: &TypeSubstitution,
    ) -> TypeId {
        let signature = AppliedCallSignature {
            params: function
                .parameters()
                .map(|(_, ty)| {
                    self.aggregates
                        .normalize_type(&ty.instantiate(substitution))
                })
                .collect(),
            return_type: self
                .aggregates
                .normalize_type(&function.return_type().instantiate(substitution)),
        };
        let result = signature.return_type.clone();
        self.type_table.insert_call_signature(site, signature);
        result
    }

    pub(super) fn record_protocol_application(
        &mut self,
        site: ExprId,
        receiver: &TypeId,
        return_type: TypeId,
    ) {
        let Some(call) = self.type_table.call_resolution(site) else {
            return;
        };
        let CallTarget::TraitMethod { method, interface } = &call.target else {
            return;
        };
        let Some(method) = self.aggregates.trait_method(method) else {
            return;
        };
        let Some(owner) = self.aggregates.trait_(&method.owner) else {
            return;
        };
        let mut substitution: TypeSubstitution = owner
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.extend(
            method
                .generic_params
                .iter()
                .skip(owner.generic_params.len())
                .cloned()
                .zip(call.type_arguments),
        );
        substitution.insert_receiver(owner.id.clone(), receiver.clone());
        self.type_table.insert_call_signature(
            site,
            AppliedCallSignature {
                params: method
                    .params
                    .iter()
                    .map(|parameter| {
                        self.aggregates.normalize_type(
                            &parameter
                                .ty
                                .instantiate(&substitution)
                                .with_associated_types(interface),
                        )
                    })
                    .collect(),
                return_type,
            },
        );
    }

    pub(super) fn check_generic_call_bounds(
        &mut self,
        _parameters: &[GenericParameterType],
        bounds: &GenericBounds,
        substitution: &TypeSubstitution,
        env: &BodyTypeEnv,
        callee: ExprId,
    ) {
        for (target, constraints) in bounds {
            let actual_owned = self
                .aggregates
                .normalize_type(&target.instantiate(substitution));
            let actual = &actual_owned;
            for constraint in constraints.iter().cloned() {
                match constraint {
                    ConstraintTarget::Standard(constraint) => {
                        self.check_standard_constraint(actual, constraint, env, callee)
                    }
                    ConstraintTarget::Trait(trait_type) => {
                        let trait_type = trait_type.instantiate(substitution);
                        self.constrain_declared_bound(actual, &trait_type);
                        let satisfied = self.aggregates.intrinsic_implementation(
                            &trait_type,
                            actual,
                            &env.generic_bounds,
                        ) || match actual {
                            TypeId::Generic(_)
                            | TypeId::SelfType(_)
                            | TypeId::Projection { .. } => self
                                .trait_bounds_for(actual, env)
                                .iter()
                                .any(|bound| bound.satisfies(&trait_type)),
                            _ => match self.aggregates.implementation_count(&trait_type, actual)
                                + usize::from(
                                    self.declarations.hosts.implements(&trait_type, actual),
                                ) {
                                0 => {
                                    StandardTrait::from_id(&trait_type.declaration).is_none()
                                        && self.type_table.implements(&trait_type, actual)
                                }
                                1 => true,
                                _ => false,
                            },
                        };
                        if !satisfied && !actual.is_unresolved() {
                            let trait_name = self
                                .aggregates
                                .trait_(&trait_type.declaration)
                                .map(|contract| contract.declaration.name.clone())
                                .or_else(|| {
                                    trait_type
                                        .declaration
                                        .path
                                        .last()
                                        .map(|segment| segment.name.clone())
                                })
                                .unwrap_or_default();
                            self.diagnostics.push(
                                Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                                    type_name: actual.display_name(),
                                    trait_name,
                                })
                                .with_span(self.lowered.source_map.expr_span(callee)),
                            );
                        }
                    }
                }
            }
        }
    }

    pub(super) fn check_function_arguments(
        &mut self,
        function: &impl CallableSignature,
        substitution: &TypeSubstitution,
        callee: ExprId,
        arg_tys: &[(ExprId, TypeId)],
        implicit: usize,
    ) {
        if function.parameters().len() != arg_tys.len() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: function.name().to_owned(),
                    expected: function.parameters().len().saturating_sub(implicit),
                    found: arg_tys.len().saturating_sub(implicit),
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
        for (index, (name, ty)) in function.parameters().enumerate() {
            if self.cancel.check().is_err() {
                return;
            }
            self.check_arg_type(
                function.name(),
                name,
                self.aggregates
                    .normalize_type(&ty.instantiate(substitution)),
                index,
                arg_tys,
            );
        }
    }

    pub(super) fn check_arg_type(
        &mut self,
        function_name: &str,
        parameter_name: &str,
        expected: TypeId,
        index: usize,
        args: &[(ExprId, TypeId)],
    ) {
        let Some((arg_expr, found)) = args.get(index) else {
            return;
        };
        let Ok(completes) = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            *arg_expr,
            self.cancel,
        ) else {
            return;
        };
        if !completes {
            return;
        }
        let _ = self.solver.constrain(&expected, found, self.cancel);
        if found.conflicts_with(&expected) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                    function_name: function_name.to_owned(),
                    parameter_name: parameter_name.to_owned(),
                    expected: display_type_id(&expected),
                    found: display_type_id(found),
                })
                .with_span(self.lowered.source_map.expr_span(*arg_expr)),
            );
        }
    }

    pub(super) fn emit_arg_mismatch(
        &mut self,
        function_name: &str,
        parameter_name: &str,
        expected: &TypeId,
        found: &TypeId,
        span_expr: ExprId,
    ) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::ArgumentTypeMismatch {
                function_name: function_name.to_owned(),
                parameter_name: parameter_name.to_owned(),
                expected: display_type_id(expected),
                found: display_type_id(found),
            })
            .with_span(self.lowered.source_map.expr_span(span_expr)),
        );
    }

    pub(super) fn check_standard_constraint(
        &mut self,
        ty: &TypeId,
        constraint: StandardTypeConstraint,
        env: &BodyTypeEnv,
        span_expr: ExprId,
    ) {
        if matches!(
            constraint,
            StandardTypeConstraint::HashKey | StandardTypeConstraint::Comparable
        ) {
            let protocols: &[StandardTrait] = if constraint == StandardTypeConstraint::HashKey {
                &[StandardTrait::Eq, StandardTrait::Hash]
            } else {
                &[StandardTrait::PartialEq]
            };
            if !ty.is_unresolved()
                && protocols
                    .iter()
                    .any(|p| !intrinsic_holds(*p, ty, Some(self.aggregates), &env.generic_bounds))
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                        type_name: ty.display_name(),
                        constraint: standard_surface::standard_constraint_name(constraint).into(),
                        reason: constraints::standard_constraint_reason(constraint).into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(span_expr)),
                );
            }
        } else {
            check::validate_standard_constraint_type(
                ty,
                constraint,
                &env.generic_bounds,
                self.lowered.source_map.expr_span(span_expr),
                self.diagnostics,
            );
        }
    }

    pub(super) fn string_literal_value(&self, expr_id: ExprId) -> Option<String> {
        match self.type_table.scalar_value(expr_id)? {
            ScalarValue::String(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub(super) fn finish_inferred_arguments(
        &mut self,
        substitution: &mut TypeSubstitution,
        parameters: &[GenericParameterType],
        name: &str,
        site: ExprId,
        suppress_missing: bool,
    ) -> Vec<TypeId> {
        let mut arguments = Vec::with_capacity(parameters.len());
        for (index, parameter) in parameters.iter().enumerate() {
            if self.body_inference && !suppress_missing {
                let variable = self.inference_variable(site, index + 1024);
                if let Some(inferred) = substitution.get(parameter) {
                    let _ = self.solver.constrain(&variable, inferred, self.cancel);
                }
                let inferred = self.solver.resolve(&variable);
                substitution.insert(parameter.clone(), inferred);
            }
            let inferred = substitution.get(parameter);
            let unknown = inferred.is_some_and(TypeId::contains_unknown);
            if !self.solving && (unknown || (inferred.is_none() && !suppress_missing)) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                        function_name: name.to_owned(),
                        parameter: parameter.name.clone(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
            let argument = inferred
                .map(|ty| {
                    if self.solving {
                        ty.clone()
                    } else {
                        ty.diagnose_unknowns()
                    }
                })
                .unwrap_or(TypeId::Error);
            // Result facts and subsequent member/argument checks consume exactly
            // the same recovery substitution, including previously absent binders.
            substitution.insert(parameter.clone(), argument.clone());
            arguments.push(argument);
        }
        arguments
    }

    pub(super) fn infer_generic_args(
        &mut self,
        args: &[ExprId],
        parameters: impl Iterator<Item = TypeId>,
        generics: &[GenericParameterType],
        substitution: &mut TypeSubstitution,
        env: &mut BodyTypeEnv,
    ) -> Vec<(ExprId, TypeId)> {
        self.infer_bounded_args(
            args,
            parameters,
            generics,
            substitution,
            &Default::default(),
            env,
        )
    }

    pub(super) fn seed_callable_context(
        &mut self,
        site: ExprId,
        generics: &[GenericParameterType],
        bounds: &GenericBounds,
        substitution: &mut TypeSubstitution,
    ) {
        if !bounds.values().flatten().any(|bound| matches!(bound, ConstraintTarget::Trait(interface) if StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Fn))) { return; }
        // Later arguments can provide the input type of an earlier callback.
        for (index, parameter) in generics.iter().enumerate() {
            let inferred = self.inference_variable(site, index + 1024);
            if self.body_inference {
                substitution.entry(parameter.clone()).or_insert(inferred);
            }
        }
    }

    pub(super) fn infer_bounded_args(
        &mut self,
        args: &[ExprId],
        parameters: impl Iterator<Item = TypeId>,
        generics: &[GenericParameterType],
        substitution: &mut TypeSubstitution,
        bounds: &GenericBounds,
        env: &mut BodyTypeEnv,
    ) -> Vec<(ExprId, TypeId)> {
        let mut parameters = parameters.fuse();
        let mut actual = Vec::new();
        for argument in args {
            if self.cancel.check().is_err() {
                break;
            }
            let parameter = parameters.next();
            let callable = parameter
                .as_ref()
                .and_then(|parameter| bounds.get(parameter))
                .and_then(|constraints| {
                    constraints.iter().find_map(|constraint| match constraint {
                        ConstraintTarget::Trait(interface) => traits::callable_signature(interface),
                        _ => None,
                    })
                });
            let expected = callable
                .as_ref()
                .or(parameter.as_ref())
                .map(|ty| ty.argument_context(substitution, generics));
            let ty = if callable.is_some() {
                self.infer_expr_type_expected(*argument, env, expected.as_ref())
            } else {
                self.infer_expr_with_coercion(*argument, env, expected.as_ref())
            };
            let Ok(completes) = completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                self.type_table,
                *argument,
                self.cancel,
            ) else {
                break;
            };
            if completes && let Some(parameter) = parameter.as_ref() {
                let context = parameter.argument_context(substitution, generics);
                let _ = self.solver.constrain(&context, &ty, self.cancel);
            }
            if completes
                && !generics.is_empty()
                && let Some(parameter) = parameter
                && inference::infer(&parameter, &ty, generics, substitution, self.cancel).is_err()
            {
                break;
            }
            if completes && let Some(callable) = callable {
                let signature = if matches!(ty, TypeId::Function { .. }) {
                    Some(ty.clone())
                } else {
                    self.callable_contract(&ty, env)
                        .map(|(_, signature)| signature)
                };
                if let Some(signature) = signature {
                    let context = callable.argument_context(substitution, generics);
                    let _ = self.solver.constrain(&context, &signature, self.cancel);
                    let _ = inference::infer(
                        &callable,
                        &signature,
                        generics,
                        substitution,
                        self.cancel,
                    );
                }
            }
            actual.push((*argument, ty));
        }
        actual
    }

    pub(super) fn infer_typed_args(
        &mut self,
        args: &[ExprId],
        expected: impl Iterator<Item = TypeId>,
        env: &mut BodyTypeEnv,
    ) -> Vec<(ExprId, TypeId)> {
        self.infer_generic_args(args, expected, &[], &mut Default::default(), env)
    }

    pub(super) fn infer_call_args(
        &mut self,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
    ) -> Vec<(ExprId, TypeId)> {
        self.infer_typed_args(args, iter::empty(), env)
    }

    pub(super) fn check_builtin_arity(
        &mut self,
        name: &str,
        expected: usize,
        found: usize,
        callee: ExprId,
    ) {
        if expected != found {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: name.to_string(),
                    expected,
                    found,
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
    }
}
