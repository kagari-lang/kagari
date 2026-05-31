use crate::{builtin::surface, hir, types::TypeId};

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
        hir::TypeKind::Generic { name, args } => {
            let args = args
                .iter()
                .map(|arg| resolve_type(module, *arg))
                .collect::<Option<Vec<_>>>()?;
            surface::standard_enum_type(name, args)
        }
        hir::TypeKind::Tuple(elements) => elements
            .iter()
            .map(|element| resolve_type(module, *element))
            .collect::<Option<Vec<_>>>()
            .map(|elements| {
                if elements.is_empty() {
                    TypeId::from_name("()").expect("unit builtin should exist")
                } else {
                    TypeId::Tuple(elements)
                }
            }),
        hir::TypeKind::Array(element) => {
            resolve_type(module, *element).map(|element| TypeId::Array(Box::new(element)))
        }
    }
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
