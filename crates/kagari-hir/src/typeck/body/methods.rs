use crate::{
    aggregates::ImplementationSearchError,
    builtin::{
        declarations::{self, ApiImplementationSemantics, ApiTypeSemantics},
        traits::{self, StandardTraitSemantics},
    },
    hir::{ExprId, ExprKind, TypeKind},
    typeck::{
        BodyTypeEnv, CallTarget, ConstraintTarget, ResolvedAssociatedConst,
        ResolvedInterfaceCoercion, ResolvedInterfaceImplementation,
        body::BodyChecker,
        completion, inference,
        ty::{self, TypeContext, resolve_type_in},
    },
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_abi::standard::traits::StandardTrait;
use kagari_common::{Diagnostic, DiagnosticKind, identity};

impl<'a> BodyChecker<'a> {
    pub(super) fn infer_expr_with_coercion(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let source = self.infer_expr_type_expected(expr_id, env, expected);
        if expected.is_some_and(|target| source.can_weaken_to(target)) {
            let view = source.read_only_view().expect("checked collection view");
            self.type_table.insert_expr(expr_id, view.clone());
            env.exprs.insert(expr_id, view.clone());
            return view;
        }
        self.apply_interface_coercion(expr_id, source, expected, env)
    }

    pub(super) fn apply_interface_coercion(
        &mut self,
        expr_id: ExprId,
        source: TypeId,
        expected: Option<&TypeId>,
        env: &BodyTypeEnv,
    ) -> TypeId {
        if let Some(target @ TypeId::Function { .. }) = expected
            && !matches!(source, TypeId::Function { .. })
            && let Some((interface, signature)) = self.callable_contract(&source, env)
            && !signature.conflicts_with(target)
        {
            let _ = self.solver.constrain(target, &signature, self.cancel);
            self.type_table
                .insert_callable_coercion(expr_id, source, interface);
            return signature;
        }
        let Some(target @ TypeId::Trait(interface)) = expected else {
            return source;
        };
        if source == *target
            || !source.is_resolved_in(
                &env.generics
                    .iter()
                    .filter_map(|param| self.declarations.generic_type(param.id))
                    .collect::<Vec<_>>(),
            )
        {
            return source;
        }
        if let TypeId::Trait(child) = &source
            && self
                .aggregates
                .trait_closure(child, &source, self.cancel)
                .is_ok_and(|parents| parents.iter().any(|parent| parent.satisfies(interface)))
        {
            self.type_table.insert_interface_coercion(
                expr_id,
                ResolvedInterfaceCoercion {
                    implementation: ResolvedInterfaceImplementation::Upcast,
                    concrete_type: source,
                    interface_type: interface.clone(),
                },
            );
            return target.clone();
        }
        let mut native_interface = interface.clone();
        if let Some(storage) = traits::collection_storage(interface) {
            let _ = self.solver.constrain(&storage, &source, self.cancel);
            if let TypeId::Trait(resolved) = self.solver.resolve(&TypeId::Trait(interface.clone()))
            {
                native_interface = resolved;
            }
        }
        if let (Some(TypeId::Trait(expected)), Some(TypeId::Trait(actual))) = (
            TypeId::Trait(native_interface.clone()).collection_view(),
            source.collection_view(),
        ) && expected.declaration == actual.declaration
        {
            for (expected, actual) in native_interface.arguments.iter_mut().zip(&actual.arguments) {
                expected.recover_from(actual);
            }
        }
        if traits::native_interface_applies(&native_interface, &source) {
            self.type_table.insert_interface_coercion(
                expr_id,
                ResolvedInterfaceCoercion {
                    implementation: ResolvedInterfaceImplementation::Native,
                    concrete_type: source,
                    interface_type: native_interface.clone(),
                },
            );
            return TypeId::Trait(native_interface);
        }
        if matches!(&source, TypeId::Host(_))
            && self.declarations.hosts.implements(interface, &source)
        {
            self.type_table.insert_interface_coercion(
                expr_id,
                ResolvedInterfaceCoercion {
                    implementation: ResolvedInterfaceImplementation::Host,
                    concrete_type: source,
                    interface_type: interface.clone(),
                },
            );
            return target.clone();
        }
        match self.aggregates.concrete_interface_implementation(
            interface,
            &source,
            &env.generic_bounds,
            4096,
            64,
            self.cancel,
        ) {
            Ok(Some((implementation, arguments))) => {
                self.type_table.insert_interface_coercion(
                    expr_id,
                    ResolvedInterfaceCoercion {
                        implementation: ResolvedInterfaceImplementation::Script {
                            declaration: implementation,
                            arguments,
                        },
                        concrete_type: source,
                        interface_type: interface.clone(),
                    },
                );
                target.clone()
            }
            Ok(None) | Err(ImplementationSearchError::Cancelled) => source,
            Err(ImplementationSearchError::LimitExceeded) => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                        resource: "interface implementation search",
                        limit: 4096,
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                source
            }
        }
    }

    pub(super) fn infer_inherent_method_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let ExprKind::Field { receiver, name } = &self.lowered.module.expr(callee).kind else {
            return None;
        };
        let receiver = *receiver;
        let name = name.clone();
        let receiver_ty = self.infer_expr_type(receiver, env);
        if receiver_ty.is_unresolved() {
            return None;
        }
        let candidates = self
            .aggregates
            .inherent_methods()
            .filter(|method| {
                method.function.name == name
                    && method.visibility.allows(
                        &method.declaration.module,
                        self.lowered.source.module_identity(),
                    )
            })
            .filter_map(|method| {
                let function = method.function.clone();
                let mut substitution = TypeSubstitution::default();
                let generics = function.generic_params.as_slice();
                if inference::infer(
                    &method.owner,
                    &receiver_ty,
                    generics,
                    &mut substitution,
                    self.cancel,
                )
                .is_err()
                {
                    return None;
                }
                let target = if method.id.file == self.lowered.source.id()
                    && method.id.revision == self.lowered.source.revision()
                {
                    CallTarget::Function(method.id.function)
                } else {
                    CallTarget::SourceFunction(method.id)
                };
                Some((function, substitution, target))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        if candidates.len() != 1 {
            self.infer_call_args(args, env);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::AmbiguousMethod { name })
                    .with_span(self.lowered.source_map.expr_span(callee)),
            );
            return Some(TypeId::Error);
        }
        let (function, mut substitution, target) =
            candidates.into_iter().next().expect("one method");
        if matches!(target, CallTarget::SourceFunction(_)) && !function.generic_params.is_empty() {
            self.infer_call_args(args, env);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PublicGenericFunction {
                    name: function.name.clone(),
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
            return Some(TypeId::Error);
        }
        let explicit_parameters = function
            .generic_params
            .iter()
            .filter(|parameter| !substitution.contains_key(*parameter))
            .cloned()
            .collect::<Vec<_>>();
        self.seed_callable_context(
            callee,
            &function.generic_params,
            &function.bounds,
            &mut substitution,
        );
        self.seed_explicit_arguments(callee, &explicit_parameters, &mut substitution);
        if let Some(expected) = expected
            && inference::infer(
                &function.return_type,
                expected,
                &function.generic_params,
                &mut substitution,
                self.cancel,
            )
            .is_err()
        {
            return Some(TypeId::Unknown);
        }
        self.type_table
            .insert_call(call_expr, target, Some(receiver));
        let arg_tys = self.infer_bounded_args(
            args,
            function
                .params
                .iter()
                .skip(1)
                .map(|parameter| parameter.ty.clone()),
            &function.generic_params,
            &mut substitution,
            &function.bounds,
            env,
        );
        let suppress_missing = arg_tys.iter().any(|(_, ty)| ty.is_unresolved());
        let type_arguments = self.finish_inferred_arguments(
            &mut substitution,
            &function.generic_params,
            &function.name,
            callee,
            suppress_missing,
        );
        self.type_table
            .insert_type_arguments(call_expr, type_arguments);
        self.check_generic_call_bounds(
            &function.generic_params,
            &function.bounds,
            &substitution,
            env,
            callee,
        );
        let mut all_args = Vec::with_capacity(arg_tys.len() + 1);
        all_args.push((receiver, receiver_ty));
        all_args.extend(arg_tys);
        self.check_function_arguments(&function, &substitution, callee, &all_args);
        Some(
            self.aggregates
                .normalize_type(&function.return_type.instantiate(&substitution)),
        )
    }

    pub(super) fn infer_associated_const(
        &mut self,
        expr: ExprId,
        env: &BodyTypeEnv,
    ) -> Option<TypeId> {
        let ExprKind::Name {
            name,
            explicit_type,
        } = &self.lowered.module.expr(expr).kind
        else {
            return None;
        };
        let context = TypeContext {
            declarations: self.declarations,
            generics: &env.generics,
            self_type: None,
            implementation: None,
        };
        let qualified = explicit_type.and_then(|id| match &self.lowered.module.type_ref(id).kind {
            TypeKind::Projection {
                receiver,
                trait_ref,
                member,
                arguments,
            } if arguments.is_empty() => Some((*receiver, *trait_ref, member.clone())),
            _ => None,
        });
        let (receiver, member, requested) = if let Some((receiver, trait_ref, member)) = qualified {
            let receiver = if matches!(&self.lowered.module.type_ref(receiver).kind, crate::hir::TypeKind::Named(name) if name == "Self")
            {
                env.self_type.clone().unwrap_or(TypeId::Error)
            } else {
                resolve_type_in(
                    &self.lowered.module,
                    receiver,
                    context,
                    self.type_table,
                    self.cancel,
                )
            };
            let interface = resolve_type_in(
                &self.lowered.module,
                trait_ref,
                context,
                self.type_table,
                self.cancel,
            );
            (receiver, member, Some(interface))
        } else {
            let (owner, member) = name.rsplit_once("::")?;
            let receiver = if let Some(ty) = explicit_type {
                resolve_type_in(
                    &self.lowered.module,
                    *ty,
                    context,
                    self.type_table,
                    self.cancel,
                )
            } else if owner == "Self" {
                env.self_type.clone().unwrap_or(TypeId::Error)
            } else {
                ty::resolve_named_type(owner, context).ty
            };
            (receiver, member.to_owned(), None)
        };
        if receiver.is_unresolved() {
            return None;
        }
        let mut candidates = Vec::new();
        for interface in self.trait_bounds_for(&receiver, env) {
            if requested.as_ref().is_some_and(|requested| !matches!(requested, TypeId::Trait(required) if interface.satisfies(required))) { continue; }
            let id = identity::associated_const_id(&interface.declaration, &member);
            if let Some(signature) = self
                .aggregates
                .trait_(&interface.declaration)
                .and_then(|contract| contract.associated_consts.get(&id))
                && !candidates
                    .iter()
                    .any(|(candidate, _, _)| candidate == &interface)
            {
                candidates.push((interface, id, signature.ty.clone()));
            }
        }
        if candidates.len() == 1 {
            let (interface, member, ty) = candidates.remove(0);
            self.type_table.insert_associated_const(
                expr,
                ResolvedAssociatedConst {
                    receiver,
                    interface,
                    member,
                },
            );
            return Some(ty);
        }
        if candidates.len() > 1 || requested.is_some() {
            self.diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidAssociatedConst { name: member,
                reason: "requires a unique constant from a satisfied trait; use a qualified path to disambiguate".into(),
            }).with_span(self.lowered.source_map.expr_span(expr)));
            return Some(TypeId::Error);
        }
        None
    }

    pub(super) fn trait_bounds_for(&self, ty: &TypeId, env: &BodyTypeEnv) -> Vec<NominalType> {
        if let TypeId::Trait(interface) = ty {
            let mut bounds = self
                .aggregates
                .trait_closure(interface, ty, self.cancel)
                .unwrap_or_default();
            if StandardTrait::from_id(&interface.declaration).is_some_and(StandardTrait::collection)
            {
                bounds.extend(
                    [
                        StandardTrait::PartialEq,
                        StandardTrait::Eq,
                        StandardTrait::Hash,
                        StandardTrait::Debug,
                    ]
                    .map(|kind| kind.nominal()),
                );
            }
            return bounds;
        }
        if !matches!(
            ty,
            TypeId::Generic(_) | TypeId::SelfType(_) | TypeId::Projection { .. }
        ) {
            let mut implemented = Vec::new();
            for implementation in self.aggregates.implementations() {
                let mut substitution = TypeSubstitution::default();
                if inference::infer(
                    &implementation.for_type,
                    ty,
                    &implementation.generic_params,
                    &mut substitution,
                    self.cancel,
                )
                .is_ok()
                {
                    let applied = implementation.trait_type.instantiate(&substitution);
                    if matches!(
                        self.aggregates.concrete_interface_implementation(
                            &applied,
                            ty,
                            &env.generic_bounds,
                            100_000,
                            64,
                            self.cancel
                        ),
                        Ok(Some(_))
                    ) && !implemented.contains(&applied)
                    {
                        implemented.push(applied);
                    }
                }
            }
            for implementation in declarations::implementations(ty) {
                let declaration = implementation.trait_declaration().item.identity();
                if !StandardTrait::from_id(&declaration).is_some_and(StandardTrait::collection) {
                    continue;
                }
                if let Some(arguments) = implementation.arguments(ty) {
                    let applied = NominalType {
                        declaration,
                        arguments: implementation
                            .trait_arguments
                            .iter()
                            .map(|t| t.instantiate(&arguments))
                            .collect(),
                        associated_types: Default::default(),
                    };
                    if !implemented.contains(&applied) {
                        implemented.push(applied);
                    }
                }
            }
            for kind in StandardTrait::ALL {
                let interface = kind.intrinsic_view(ty);
                if traits::intrinsic_holds(kind, ty, Some(self.aggregates), &env.generic_bounds)
                    && !implemented.contains(&interface)
                {
                    implemented.push(interface);
                }
            }
            if !implemented.is_empty() {
                self.add_iterator_view(ty, &mut implemented);
                return implemented;
            }
        }
        let mut bounds = env
            .generic_bounds
            .get(ty)
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        if let TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = ty
            && let Some(contract) = self.aggregates.trait_(&interface.declaration)
        {
            let mut applied = (**interface).clone();
            for bound in env
                .generic_bounds
                .get(receiver.as_ref())
                .into_iter()
                .flatten()
            {
                if let ConstraintTarget::Trait(available) = bound
                    && available.declaration == applied.declaration
                    && available.arguments == applied.arguments
                {
                    applied
                        .associated_types
                        .extend(available.associated_types.clone());
                }
            }
            let mut substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(interface.arguments.iter().cloned())
                .collect();
            substitution.insert_receiver(interface.declaration.clone(), (**receiver).clone());
            if let Some(inputs) = contract.associated_type_parameters.get(member) {
                substitution.extend(
                    inputs
                        .parameters
                        .iter()
                        .cloned()
                        .zip(arguments.iter().cloned()),
                );
            }
            bounds.extend(
                contract
                    .associated_types
                    .get(member)
                    .into_iter()
                    .flatten()
                    .map(|bound| match bound {
                        ConstraintTarget::Standard(value) => ConstraintTarget::Standard(*value),
                        ConstraintTarget::Trait(value) => {
                            let TypeId::Trait(value) = TypeId::Trait(value.clone())
                                .instantiate(&substitution)
                                .with_associated_types(&applied)
                            else {
                                unreachable!("associated trait bound");
                            };
                            ConstraintTarget::Trait(value)
                        }
                    }),
            );
        }
        let direct = bounds
            .into_iter()
            .filter_map(|bound| match bound {
                ConstraintTarget::Trait(ty) => Some(ty),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut expanded = Vec::new();
        for interface in direct {
            for parent in self
                .aggregates
                .trait_closure(&interface, ty, self.cancel)
                .unwrap_or_default()
            {
                if !expanded.contains(&parent) {
                    expanded.push(parent);
                }
            }
        }
        self.add_iterator_view(ty, &mut expanded);
        expanded
    }

    pub(super) fn infer_trait_method_call_type(
        &mut self,
        call_expr: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let expr = self.lowered.module.expr(callee);
        let ExprKind::Field { receiver, name } = &expr.kind else {
            return None;
        };
        let receiver_ty = self.infer_expr_type(*receiver, env);
        let mut trait_types = self.trait_bounds_for(&receiver_ty, env);
        // Arrays support every builtin integer index type. Method selection must
        // apply Index to the actual argument, just like bracket expressions.
        if matches!(receiver_ty, TypeId::Array(_, _)) && name == "index" && args.len() == 1 {
            let index_ty = self.infer_expr_type(args[0], env);
            let protocol = StandardTrait::Index;
            let mut requested = protocol.nominal();
            requested.arguments.push(index_ty);
            if let Some((interface, _)) = self.select_operator(&receiver_ty, requested, env) {
                trait_types.retain(|candidate| candidate.declaration != interface.declaration);
                trait_types.push(interface);
            }
        }
        let mut candidates = Vec::new();
        for interface in trait_types {
            if let Some(contract) = self.aggregates.trait_(&interface.declaration) {
                for method in &contract.methods {
                    if method.name == *name
                        && method
                            .params
                            .first()
                            .is_some_and(|parameter| parameter.name == "self")
                        && !candidates.contains(&(method.id.clone(), interface.clone()))
                    {
                        candidates.push((method.id.clone(), interface.clone()));
                    }
                }
            }
        }
        if candidates.is_empty() {
            return None;
        }
        // Applied standard protocols can select a unique RHS implementation.
        // Unrelated same-named traits retain ordinary ambiguity diagnostics.
        if candidates.len() > 1
            && args.len() == 1
            && let Some(protocol) = StandardTrait::from_id(&candidates[0].1.declaration)
            && (protocol.binary_operator() || protocol == StandardTrait::Index)
            && candidates
                .iter()
                .all(|(_, interface)| interface.declaration == protocol.contract().id)
        {
            let argument = self.infer_expr_type(args[0], env);
            let applicable: Vec<_> = candidates
                .iter()
                .filter(|(_, interface)| {
                    interface
                        .arguments
                        .first()
                        .is_some_and(|input| !input.conflicts_with(&argument))
                })
                .cloned()
                .collect();
            if !applicable.is_empty() {
                candidates = applicable;
            }
        }
        if candidates.len() != 1 {
            self.infer_call_args(args, env);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::AmbiguousMethod { name: name.clone() })
                    .with_span(self.lowered.source_map.expr_span(callee)),
            );
            return Some(TypeId::Error);
        }
        let method = self
            .aggregates
            .trait_method(&candidates[0].0)
            .expect("catalog method")
            .clone();
        let interface = &candidates[0].1;
        let trait_contract = self
            .aggregates
            .trait_(&interface.declaration)
            .expect("catalog trait");
        let mut substitution: TypeSubstitution = trait_contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        let self_owner = &method.owner;
        let self_ty = receiver_ty;
        substitution.insert_receiver(self_owner.clone(), self_ty.clone());
        let method_generics = &method.generic_params[trait_contract.generic_params.len()..];
        self.seed_callable_context(callee, method_generics, &method.bounds, &mut substitution);
        self.seed_explicit_arguments(callee, method_generics, &mut substitution);
        let return_pattern = method
            .return_type
            .with_self(self_owner, &self_ty)
            .instantiate(&substitution)
            .with_associated_types(interface);
        if let Some(expected) = expected
            && inference::infer(
                &return_pattern,
                expected,
                method_generics,
                &mut substitution,
                self.cancel,
            )
            .is_err()
        {
            return Some(TypeId::Unknown);
        }
        self.type_table.insert_call(
            call_expr,
            CallTarget::TraitMethod {
                method: method.id.clone(),
                interface: interface.clone(),
            },
            Some(*receiver),
        );
        let params = method
            .params
            .iter()
            .filter(|param| param.name != "self")
            .collect::<Vec<_>>();
        let param_types = params
            .iter()
            .map(|param| {
                self.aggregates.normalize_type(
                    &param
                        .ty
                        .with_self(self_owner, &self_ty)
                        .instantiate(&substitution)
                        .with_associated_types(interface),
                )
            })
            .collect::<Vec<_>>();
        let arg_tys = self.infer_bounded_args(
            args,
            param_types.iter().cloned(),
            method_generics,
            &mut substitution,
            &method.bounds,
            env,
        );
        let mut suppress_missing = arg_tys.iter().any(|(_, ty)| ty.is_unresolved());
        for (argument, _) in &arg_tys {
            let Ok(completes) = completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                *argument,
                self.cancel,
            ) else {
                return Some(TypeId::Unknown);
            };
            suppress_missing |= !completes;
        }
        let type_arguments = self.finish_inferred_arguments(
            &mut substitution,
            method_generics,
            &method.name,
            callee,
            suppress_missing,
        );
        self.type_table
            .insert_type_arguments(call_expr, type_arguments);
        let method_bounds = method
            .bounds
            .iter()
            .map(|(ty, constraints)| {
                (
                    ty.with_self(self_owner, &self_ty)
                        .instantiate(&substitution)
                        .with_associated_types(interface),
                    constraints
                        .iter()
                        .map(|constraint| match constraint {
                            ConstraintTarget::Standard(standard) => {
                                ConstraintTarget::Standard(*standard)
                            }
                            ConstraintTarget::Trait(bound) => {
                                let TypeId::Trait(bound) = self.aggregates.normalize_type(
                                    &TypeId::Trait(bound.clone())
                                        .with_self(self_owner, &self_ty)
                                        .instantiate(&substitution)
                                        .with_associated_types(interface),
                                ) else {
                                    unreachable!("trait method bound");
                                };
                                ConstraintTarget::Trait(bound)
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        self.check_generic_call_bounds(method_generics, &method_bounds, &substitution, env, callee);
        if params.len() != arg_tys.len() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: name.clone(),
                    expected: params.len(),
                    found: arg_tys.len(),
                })
                .with_span(self.lowered.source_map.expr_span(callee)),
            );
        }
        for (index, param) in params.iter().enumerate() {
            if self.cancel.check().is_err() {
                return Some(TypeId::Unknown);
            }
            self.check_arg_type(
                name,
                &param.name,
                param_types[index].instantiate(&substitution),
                index,
                &arg_tys,
            );
        }

        Some(
            self.aggregates
                .normalize_type(&return_pattern.instantiate(&substitution)),
        )
    }
}
