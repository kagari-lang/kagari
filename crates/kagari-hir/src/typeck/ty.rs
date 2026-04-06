use crate::{hir, types::TypeId};

pub(super) fn resolve_type(module: &hir::Module, ty: hir::TypeRefId) -> Option<TypeId> {
    match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => TypeId::from_name(name),
        hir::TypeKind::Tuple(_) | hir::TypeKind::Array(_) => None,
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

pub(crate) fn display_type_id(ty: TypeId) -> &'static str {
    ty.display_name()
}
