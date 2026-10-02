//! Resolve static and receiver syntax to the same checked trait call path.
use crate::{
    hir::{expr::ExprKind, ids::ExprId, ty::TypeKind},
    typeck::{
        BodyTypeEnv,
        body::BodyChecker,
        ty::{self, TypeContext, resolve_type_in},
    },
    types::{NominalType, TypeId},
};

pub(super) struct TraitCallSite {
    pub receiver: Option<ExprId>,
    pub receiver_type: TypeId,
    pub name: String,
    pub interface: Option<NominalType>,
}

impl BodyChecker<'_> {
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
                (!receiver_type.is_unresolved() && !matches!(receiver_type, TypeId::Trait(_)))
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
