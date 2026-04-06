use crate::{hir, types::TypeId};

pub(super) fn resolve_type(module: &hir::Module, ty: hir::TypeRefId) -> Option<TypeId> {
    match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => TypeId::from_name(name)
            .or_else(|| {
                module
                    .structs
                    .iter()
                    .find(|item| item.name == *name)
                    .map(|_| TypeId::Struct(name.clone()))
            })
            .or_else(|| {
                module
                    .enums
                    .iter()
                    .find(|item| item.name == *name)
                    .map(|_| TypeId::Enum(name.clone()))
            }),
        hir::TypeKind::Tuple(elements) => elements
            .iter()
            .map(|element| resolve_type(module, *element))
            .collect::<Option<Vec<_>>>()
            .map(TypeId::Tuple),
        hir::TypeKind::Array(element) => {
            resolve_type(module, *element).map(|element| TypeId::Array(Box::new(element)))
        }
    }
}

pub(super) fn display_type(module: &hir::Module, ty: hir::TypeRefId) -> String {
    match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => name.clone(),
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
