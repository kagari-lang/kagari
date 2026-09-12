use super::{ResolvedTypeRef, TypeTable, TypeTarget};
use crate::{builtin::surface, hir, types::TypeId};
use kagari_common::cancellation::CancellationToken;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct TypeContext<'a> {
    pub generics: &'a [hir::GenericParam],
    pub self_type: Option<hir::TraitId>,
}

pub(super) fn resolve_type(
    module: &hir::Module,
    ty: hir::TypeRefId,
    table: &mut TypeTable,
    cancel: &CancellationToken,
) -> Option<TypeId> {
    resolve_type_in(module, ty, TypeContext::default(), table, cancel)
}

pub(super) fn resolve_type_in(
    module: &hir::Module,
    ty: hir::TypeRefId,
    context: TypeContext<'_>,
    table: &mut TypeTable,
    cancel: &CancellationToken,
) -> Option<TypeId> {
    if cancel.check().is_err() {
        return None;
    }
    let mut target = None;
    let resolved = match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => {
            if let Some(param) = context
                .generics
                .iter()
                .rev()
                .find(|param| param.name == *name)
            {
                target = Some(TypeTarget::Generic(param.id));
                Some(TypeId::Generic(name.clone()))
            } else if name == "Self" && context.self_type.is_some() {
                target = context.self_type.map(TypeTarget::Trait);
                Some(TypeId::Generic(name.clone()))
            } else if let Some(ty) = TypeId::from_name(name) {
                Some(ty)
            } else if let Some(item) = module.structs.iter().find(|item| item.name == *name) {
                target = Some(TypeTarget::Struct(item.id));
                Some(TypeId::Struct(name.clone()))
            } else if let Some(item) = module.enums.iter().find(|item| item.name == *name) {
                target = Some(TypeTarget::Enum(item.id));
                Some(TypeId::Enum(name.clone()))
            } else if let Some(item) = module.traits.iter().find(|item| item.name == *name) {
                target = Some(TypeTarget::Trait(item.id));
                Some(TypeId::Trait(name.clone()))
            } else {
                None
            }
        }
        hir::TypeKind::Generic { name, args } => {
            // Visit every argument even if an earlier one cannot resolve.
            let args = args
                .iter()
                .map(|arg| resolve_type_in(module, *arg, context, table, cancel))
                .collect::<Vec<_>>();
            args.into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|args| surface::standard_generic_type(name, args))
        }
        hir::TypeKind::Tuple(elements) => {
            let elements = elements
                .iter()
                .map(|element| resolve_type_in(module, *element, context, table, cancel))
                .collect::<Vec<_>>();
            elements
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(|elements| {
                    if elements.is_empty() {
                        TypeId::from_name("()").expect("unit builtin")
                    } else {
                        TypeId::Tuple(elements)
                    }
                })
        }
        hir::TypeKind::Array(element) => resolve_type_in(module, *element, context, table, cancel)
            .map(|element| TypeId::Array(Box::new(element))),
    };
    table.insert_type_ref(
        ty,
        ResolvedTypeRef {
            ty: resolved.clone().unwrap_or(TypeId::Error),
            target,
        },
    );
    resolved
}
pub(super) fn display_type(module: &hir::Module, ty: hir::TypeRefId) -> String {
    match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => name.clone(),
        hir::TypeKind::Generic { name, args } => {
            let inner = args
                .iter()
                .map(|arg| display_type(module, *arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{inner}>")
        }
        hir::TypeKind::Tuple(elements) => {
            let inner = elements
                .iter()
                .map(|element| display_type(module, *element))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
        hir::TypeKind::Array(element) => format!("[{}]", display_type(module, *element)),
    }
}

pub(crate) fn display_type_id(ty: &TypeId) -> String {
    ty.display_name()
}
