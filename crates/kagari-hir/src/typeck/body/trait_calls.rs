//! Resolve static and receiver syntax to the same checked trait call path.
use crate::{
    aggregates::implementations::ImplementationSearchError,
    hir::{expr::ExprKind, ids::ExprId, ty::TypeKind},
    typeck::{
        BodyTypeEnv,
        body::BodyChecker,
        inference,
        ty::{self, TypeContext, resolve_type_in},
    },
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::language::Protocol;

pub(super) struct TraitCallSite {
    pub receiver: Option<ExprId>,
    pub receiver_type: TypeId,
    pub name: String,
    pub interface: Option<NominalType>,
}

impl BodyChecker<'_> {
    /// Refine receiver-only discovery using the named trait method's signature.
    /// For example, `fn deliver(self) -> Packet<D>` requests `Reverse<Receipt>`
    /// in a `Packet<Receipt>` context. A caller's `D` is a valid argument, but an
    /// unbound trait parameter, recovery type or missing impl is never evidence.
    pub(super) fn infer_method_interfaces(
        &mut self,
        site: &TraitCallSite,
        args: &[ExprId],
        expected: Option<&TypeId>,
        env: &BodyTypeEnv,
        available: &mut Vec<NominalType>,
        callee: ExprId,
    ) {
        if site.receiver_type.is_unresolved() || site.receiver_type.is_never() {
            return;
        }
        for contract in self.aggregates.traits() {
            if self.cancelled() {
                return;
            }
            if contract.generic_params.is_empty() {
                continue;
            }
            let Some(method) = contract.methods.iter().find(|m| {
                m.name == site.name
                    && m.params.first().is_some_and(|p| p.name == "self") == site.receiver.is_some()
            }) else {
                continue;
            };
            // Candidate discovery reads expression facts from prior solving rounds.
            // Never type an argument with a speculative signature: an unrelated
            // candidate could otherwise commit the wrong literal/closure context.
            let cancel = self.cancel;
            let inputs = method
                .params
                .iter()
                .skip(usize::from(site.receiver.is_some()))
                .zip(args)
                .map(|(param, &arg)| (&param.ty, self.inference_variable(arg, 0)));
            let mut substitution = TypeSubstitution::default();
            for (pattern, actual) in expected
                .cloned()
                .map(|ty| (&method.return_type, ty))
                .into_iter()
                .chain(inputs)
            {
                if !actual.is_never()
                    && inference::infer(
                        pattern,
                        &actual,
                        &contract.generic_params,
                        &mut substitution,
                        cancel,
                        None,
                    )
                    .is_err()
                {
                    self.stopped = true;
                    return;
                }
            }
            let Some(arguments) = contract
                .generic_params
                .iter()
                .map(|p| substitution.get(p).filter(|t| !t.is_unresolved()).cloned())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let requested = NominalType {
                declaration: contract.id,
                arguments,
                associated_types: Default::default(),
            };
            if available.iter().any(|n| n.satisfies(&requested)) {
                available.retain(|n| n.declaration != contract.id || n.satisfies(&requested));
            } else if let Some(applied) =
                self.prove_declared_call_interface(callee, &site.receiver_type, &requested, env)
            {
                available.retain(|n| n.declaration != contract.id);
                available.push(applied);
            }
        }
    }

    /// Return the fully substituted selected impl header, not the requested pattern.
    fn prove_declared_call_interface(
        &mut self,
        callee: ExprId,
        receiver: &TypeId,
        requested: &NominalType,
        env: &BodyTypeEnv,
    ) -> Option<NominalType> {
        match self.aggregates.selected_declared_interface(
            self.declarations.definitions(),
            requested,
            receiver,
            &env.generic_bounds,
            self.cancel,
        ) {
            Ok(applied) => applied,
            Err(ImplementationSearchError::Cancelled) => {
                self.stopped = true;
                None
            }
            Err(ImplementationSearchError::LimitExceeded) => {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                        resource: "trait implementation search",
                        limit: 4096,
                    })
                    .with_span(self.lowered.module.expr(callee).origin.diagnostic_span()),
                );
                None
            }
        }
    }

    /// Constrain trait-owned Self from the declared result, without consulting impls.
    pub(super) fn infer_trait_owned_self(
        &mut self,
        callee: ExprId,
        interface: &NominalType,
        name: &str,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let contract = self.aggregates.trait_(&interface.declaration)?;
        let method = contract.methods.iter().find(|method| {
            method.name == name
                && !method
                    .params
                    .first()
                    .is_some_and(|param| param.name == "self")
        })?;
        let method_generics = &method.generic_params[contract.generic_params.len()..];
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        for (index, parameter) in method_generics.iter().enumerate() {
            let variable = self.inference_variable(callee, index + 1024);
            substitution.insert(parameter.clone(), variable);
        }
        if let Some(arguments) = self.explicit_arguments.get(&callee) {
            substitution.extend(
                method_generics
                    .iter()
                    .cloned()
                    .zip(arguments.iter().cloned()),
            );
        }
        // This slot belongs to the static callee, not the call result or a method binder.
        let self_ty = self.inference_variable(callee, 2);
        let result = method
            .return_type
            .with_self(&method.owner, &self_ty)
            .instantiate(&substitution)
            .with_associated_types(interface, &self_ty);
        if let Some(expected) = expected
            && !expected.is_never()
        {
            self.constrain_types(&result, expected);
        }
        Some(self.solver.resolve(&self_ty))
    }

    /// Select a qualified application after resolving (or contextually inferring)
    /// its receiver. The qualification is a request, never proof of membership.
    pub(super) fn select_static_trait_interface(
        &mut self,
        callee: ExprId,
        receiver: &TypeId,
        interface: &NominalType,
        env: &BodyTypeEnv,
        name: &str,
    ) -> Option<NominalType> {
        if receiver.is_unresolved() || receiver.is_never() {
            if !self.solving {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                        function_name: name.to_owned(),
                        parameter: "Self".into(),
                    })
                    .with_span(self.lowered.module.expr(callee).origin.diagnostic_span()),
                );
            }
            return None;
        }
        if !matches!(receiver, TypeId::Trait(_))
            && let Some(applied) = self
                .trait_bounds_for(receiver, env)
                .into_iter()
                .find(|bound| bound.satisfies(interface))
                .or_else(|| self.prove_declared_call_interface(callee, receiver, interface, env))
                .or_else(|| {
                    let contract = self.aggregates.trait_(&interface.declaration)?;
                    if contract.conversion_adapter.is_none()
                        && Protocol::from_definition(&contract.id, self.declarations.definitions())
                            .is_none()
                    {
                        return None;
                    }
                    self.select_operator(receiver, interface.clone(), env)
                        .map(|(applied, _)| applied)
                        .filter(|applied| applied.satisfies(interface))
                })
        {
            return Some(applied);
        }
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                type_name: receiver.display_name(self.declarations.definitions()),
                trait_name: TypeId::Trait(interface.clone())
                    .display_name(self.declarations.definitions()),
            })
            .with_span(self.lowered.module.expr(callee).origin.diagnostic_span()),
        );
        None
    }

    pub(super) fn trait_call_site(
        &mut self,
        callee: ExprId,
        env: &mut BodyTypeEnv,
    ) -> Option<TraitCallSite> {
        match &self.lowered.module.expr(callee).kind {
            ExprKind::Field { receiver, name } => Some(TraitCallSite {
                receiver: Some(*receiver),
                receiver_type: self.infer_expr_type(*receiver, env),
                name: name.clone(),
                interface: None,
            }),
            ExprKind::Name {
                name,
                explicit_type,
            } => {
                if self.names.expr_resolution(callee).is_some()
                    || self.associated_owner_shadowed(callee)
                    || self.associated_function(callee).is_some()
                {
                    return None;
                }
                let context = TypeContext {
                    declarations: self.declarations,
                    generics: &env.generics,
                    self_type: None,
                    implementation: None,
                };
                let resolve = |id, table: &mut _| {
                    if matches!(&self.lowered.module.type_ref(id).kind, TypeKind::Named(name) if name == "Self")
                    {
                        env.self_type.clone().unwrap_or(TypeId::Error)
                    } else {
                        resolve_type_in(&self.lowered.module, id, context, table, self.cancel)
                    }
                };
                let (receiver_type, name, interface) = match explicit_type
                    .map(|id| &self.lowered.module.type_ref(id).kind)
                {
                    Some(TypeKind::Projection {
                        receiver,
                        trait_ref,
                        member,
                        arguments,
                    }) if arguments.is_empty() => {
                        let receiver = resolve(*receiver, self.type_table);
                        let TypeId::Trait(interface) = resolve(*trait_ref, self.type_table) else {
                            return None;
                        };
                        (receiver, member.clone(), Some(interface))
                    }
                    _ => {
                        let (owner, member) = name.rsplit_once("::")?;
                        let receiver = if let Some(id) = explicit_type {
                            resolve(*id, self.type_table)
                        } else if owner == "Self" {
                            env.self_type.clone().unwrap_or(TypeId::Error)
                        } else {
                            ty::resolve_named_type(&self.lowered.module, owner, context).ty
                        };
                        (receiver, member.to_owned(), None)
                    }
                };
                (!receiver_type.is_unresolved()
                    && (interface.is_none() || !matches!(receiver_type, TypeId::Trait(_))))
                .then_some(TraitCallSite {
                    receiver: None,
                    receiver_type,
                    name,
                    interface,
                })
            }
            _ => None,
        }
    }
}
