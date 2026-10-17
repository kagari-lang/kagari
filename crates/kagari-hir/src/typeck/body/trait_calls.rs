//! Resolve static and receiver syntax to the same checked trait call path.
use crate::{
    hir::{expr::ExprKind, ids::ExprId, ty::TypeKind},
    typeck::{
        BodyTypeEnv,
        body::BodyChecker,
        ty::{self, TypeContext, resolve_type_in},
    },
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

pub(super) struct TraitCallSite {
    pub receiver: Option<ExprId>,
    pub receiver_type: TypeId,
    pub name: String,
    pub interface: Option<NominalType>,
}

impl BodyChecker<'_> {
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
            let _ = self.solver.constrain(&result, expected, self.cancel);
        }
        Some(self.solver.resolve(&self_ty))
    }

    /// Expected types identify a receiver; only ordinary selection proves its bound.
    pub(super) fn select_contextual_trait_interface(
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
                    .with_span(self.lowered.source_map.expr_span(callee)),
                );
            }
            return None;
        }
        if !matches!(receiver, TypeId::Trait(_))
            && let Some((applied, _)) = self.select_operator(receiver, interface.clone(), env)
            && applied.satisfies(interface)
        {
            return Some(applied);
        }
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                type_name: receiver.display_name(self.declarations.definitions()),
                trait_name: TypeId::Trait(interface.clone())
                    .display_name(self.declarations.definitions()),
            })
            .with_span(self.lowered.source_map.expr_span(callee)),
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
                            ty::resolve_named_type(owner, context).ty
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
